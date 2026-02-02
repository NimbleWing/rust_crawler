use rand::seq::SliceRandom;
use std::fs::File;
use std::io::Write;

const BASE_URL: &str = "https://www.alicesw.com/";
const CATALOG_URL: &str = "https://www.alicesw.com/other/chapters/id/47686.html";
const OUTPUT_FILE: &str = "output.txt";
const TITLE_SELECTOR: &str = ".j_chapterName";
const CONTENT_SELECTOR: &str = ".read-content p";
const CHAPTER_LINK_SELECTOR: &str = ".mulu_list li a";

static USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/118.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/117.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/116.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Edge/120.0.0.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Edge/119.0.0.0",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
];

struct Crawler {
    client: reqwest::Client,
    title_selector: scraper::Selector,
    content_selector: scraper::Selector,
    chapter_link_selector: scraper::Selector,
    output_file: File,
}

impl Crawler {
    fn new(output_file: File) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            client: reqwest::Client::new(),
            title_selector: scraper::Selector::parse(TITLE_SELECTOR)?,
            content_selector: scraper::Selector::parse(CONTENT_SELECTOR)?,
            chapter_link_selector: scraper::Selector::parse(CHAPTER_LINK_SELECTOR)?,
            output_file,
        })
    }

    fn random_user_agent(&self) -> &'static str {
        USER_AGENTS.choose(&mut rand::thread_rng()).unwrap_or(&USER_AGENTS[0])
    }

    async fn fetch_page(&self, url: &str) -> Result<String, Box<dyn std::error::Error>> {
        let ua = self.random_user_agent();
        println!("Fetching URL: {} with User-Agent: {}", url, ua);
        let resp = self.client
            .get(url)
            .header("User-Agent", ua)
            .send()
            .await?
            .text()
            .await?;
        Ok(resp)
    }

    fn parse_chapter(&self, html: &str) -> Result<Chapter, Box<dyn std::error::Error>> {
        let document = scraper::Html::parse_document(html);
        let title_element = document.select(&self.title_selector).next()
            .ok_or("Chapter title not found")?;
        let chapter_title = title_element.text().collect::<Vec<_>>().join("");

        let paragraphs: Vec<String> = document.select(&self.content_selector)
            .filter_map(|p| {
                let text = p.text().collect::<Vec<_>>().join("");
                if !text.is_empty() { Some(text) } else { None }
            })
            .collect();

        Ok(Chapter { title: chapter_title, content: paragraphs })
    }

    fn parse_chapter_list(&self, html: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let document = scraper::Html::parse_document(html);
        let urls: Vec<String> = document.select(&self.chapter_link_selector)
            .filter_map(|a| a.value().attr("href"))
            .map(|href| {
                if href.starts_with("http") {
                    href.to_string()
                } else {
                    format!("{}{}", BASE_URL, href.trim_start_matches('/'))
                }
            })
            .collect();
        Ok(urls)
    }

    fn write_chapter(&mut self, chapter: &Chapter, chapter_num: usize) -> Result<(), Box<dyn std::error::Error>> {
        println!("第{}章: {}", chapter_num, chapter.title);
        let mut output = String::new();
        output.push_str(&chapter.title);
        output.push('\n');
        for para in &chapter.content {
            output.push_str(para);
            output.push('\n');
        }
        self.output_file.write_all(output.as_bytes())?;
        Ok(())
    }
}

struct Chapter {
    title: String,
    content: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_file = File::create(OUTPUT_FILE)?;
    let mut crawler = Crawler::new(output_file)?;

    println!("正在获取章节列表...");
    let catalog_html = crawler.fetch_page(CATALOG_URL).await?;
    let chapter_urls = crawler.parse_chapter_list(&catalog_html)?;
    println!("共找到{}章，开始爬取...", chapter_urls.len());

    let mut chapter_count = 0;
    for url in chapter_urls {
        let html = crawler.fetch_page(&url).await?;
        let chapter = crawler.parse_chapter(&html)?;
        chapter_count += 1;
        crawler.write_chapter(&chapter, chapter_count)?;
    }

    println!("共爬取{}章内容，已写入{}", chapter_count, OUTPUT_FILE);
    Ok(())
}