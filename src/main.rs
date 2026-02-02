use rand::seq::SliceRandom;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Semaphore;
use futures::future::join_all;

const CONCURRENT_LIMIT: usize = 15;

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
    semaphore: Arc<Semaphore>,
    output_file: File,
}

impl Crawler {
    fn new(output_file: File) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(CONCURRENT_LIMIT)),
            output_file,
        })
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
    let client = reqwest::Client::new();
    let client_arc = Arc::new(client);

    println!("正在获取章节列表...");
    let catalog_html = {
        let ua = USER_AGENTS.choose(&mut rand::thread_rng()).unwrap_or(&USER_AGENTS[0]);
        client_arc.get(CATALOG_URL)
            .header("User-Agent", ua.to_string())
            .send()
            .await?.text().await?
    };
    let chapter_urls = {
        let document = scraper::Html::parse_document(&catalog_html);
        document.select(&scraper::Selector::parse(CHAPTER_LINK_SELECTOR).unwrap())
            .filter_map(|a| a.value().attr("href"))
            .map(|href| {
                if href.starts_with("http") {
                    href.to_string()
                } else {
                    format!("{}{}", BASE_URL, href.trim_start_matches('/'))
                }
            })
            .collect::<Vec<_>>()
    };
    let total_chapters = chapter_urls.len();
    println!("共找到{}章，开始爬取（并发{}）...", total_chapters, CONCURRENT_LIMIT);

    let chapter_urls_arc = Arc::new(chapter_urls);
    let semaphore_arc = crawler.semaphore.clone();
    let title_sel = scraper::Selector::parse(TITLE_SELECTOR).unwrap();
    let content_sel = scraper::Selector::parse(CONTENT_SELECTOR).unwrap();
    let mut tasks = Vec::new();

    for index in 0..total_chapters {
        let url = chapter_urls_arc[index].clone();
        let semaphore = semaphore_arc.clone();
        let client = client_arc.clone();
        let title_sel = title_sel.clone();
        let content_sel = content_sel.clone();

        let task = tokio::spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            let ua = USER_AGENTS.choose(&mut rand::thread_rng()).unwrap_or(&USER_AGENTS[0]);

            match client.get(&url)
                .header("User-Agent", ua.to_string())
                .send()
                .await
            {
                Ok(resp) => match resp.text().await {
                    Ok(html) => {
                        let document = scraper::Html::parse_document(&html);
                        match document.select(&title_sel).next() {
                            Some(title_elem) => {
                                let chapter_title = title_elem.text().collect::<Vec<_>>().join("");
                                let paragraphs: Vec<String> = document
                                    .select(&content_sel)
                                    .filter_map(|p| {
                                        let text = p.text().collect::<Vec<_>>().join("");
                                        if !text.is_empty() { Some(text) } else { None }
                                    })
                                    .collect();
                                (index, Ok(Chapter { title: chapter_title, content: paragraphs }))
                            }
                            None => (index, Err("Chapter title not found".to_string())),
                        }
                    }
                    Err(e) => (index, Err(format!("Request failed: {}", e))),
                },
                Err(e) => (index, Err(format!("Send failed: {}", e))),
            }
        });
        tasks.push(task);
    }

    let raw_results = join_all(tasks).await;
    let mut results: Vec<_> = raw_results
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();
    results.sort_by_key(|(idx, _)| *idx);

    let mut success_count = 0;
    for (index, result) in results {
        match result {
            Ok(chapter) => {
                crawler.write_chapter(&chapter, index + 1)?;
                success_count += 1;
            }
            Err(e) => {
                println!("第{}章爬取失败: {}", index + 1, e);
            }
        }
    }

    println!("共爬取{}章内容，已写入{}章", total_chapters, success_count);
    Ok(())
}