use rand::seq::SliceRandom;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;

fn get_timestamp() -> String {
    let now = chrono::Local::now();
    now.format("[%H:%M:%S]").to_string()
}

struct ChapterResult {
    index: usize,
    title: String,
    url: String,
    content: Vec<String>,
    success: bool,
    error_msg: Option<String>,
    duration_ms: u64,
}

impl ChapterResult {
    fn success(index: usize, title: String, url: String, content: Vec<String>, duration_ms: u64) -> Self {
        ChapterResult {
            index,
            title,
            url,
            content,
            success: true,
            error_msg: None,
            duration_ms,
        }
    }

    fn failure(index: usize, url: String, error_msg: String, duration_ms: u64) -> Self {
        ChapterResult {
            index,
            title: String::new(),
            url,
            content: Vec::new(),
            success: false,
            error_msg: Some(error_msg),
            duration_ms,
        }
    }

    fn log(&self) {
        let idx = self.index + 1;
        if self.success {
            println!("{} [{}] 爬取成功: {} ({}ms)", get_timestamp(), idx, self.title, self.duration_ms);
        } else {
            println!("{} [{}] 爬取失败: {} ({})", get_timestamp(), idx, self.url, self.error_msg.as_ref().unwrap_or(&String::new()));
        }
    }
}

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
    let start_time = Instant::now();
    let output_file = File::create(OUTPUT_FILE)?;
    let mut crawler = Crawler::new(output_file)?;
    let client = reqwest::Client::new();
    let client_arc = Arc::new(client);

    println!("{} 开始获取章节列表...", get_timestamp());
    let catalog_start = Instant::now();
    let catalog_html = {
        let ua = USER_AGENTS.choose(&mut rand::thread_rng()).unwrap_or(&USER_AGENTS[0]);
        client_arc.get(CATALOG_URL)
            .header("User-Agent", ua.to_string())
            .send()
            .await?.text().await?
    };
    let catalog_duration = catalog_start.elapsed().as_millis();
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
    println!("{} 章节列表获取成功，共 {} 章 ({}ms)", get_timestamp(), total_chapters, catalog_duration);
    println!("{} 开始并发爬取（并发数: {}）", get_timestamp(), CONCURRENT_LIMIT);

    let chapter_urls_arc = Arc::new(chapter_urls);
    let semaphore_arc = crawler.semaphore.clone();
    let title_sel = scraper::Selector::parse(TITLE_SELECTOR).unwrap();
    let content_sel = scraper::Selector::parse(CONTENT_SELECTOR).unwrap();
    let mut tasks = Vec::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ChapterResult>(total_chapters);

    for index in 0..total_chapters {
        let url = chapter_urls_arc[index].clone();
        let semaphore = semaphore_arc.clone();
        let client = client_arc.clone();
        let title_sel = title_sel.clone();
        let content_sel = content_sel.clone();
        let tx = tx.clone();

        let task = tokio::spawn(async move {
            let task_start = Instant::now();
            let _permit = semaphore.acquire().await.unwrap();
            let ua = USER_AGENTS.choose(&mut rand::thread_rng()).unwrap_or(&USER_AGENTS[0]);

            let result = match client.get(&url)
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
                                ChapterResult::success(index, chapter_title, url, paragraphs, task_start.elapsed().as_millis() as u64)
                            }
                            None => ChapterResult::failure(index, url, "Chapter title not found".to_string(), task_start.elapsed().as_millis() as u64),
                        }
                    }
                    Err(e) => ChapterResult::failure(index, url, format!("Request failed: {}", e), task_start.elapsed().as_millis() as u64),
                },
                Err(e) => ChapterResult::failure(index, url, format!("Send failed: {}", e), task_start.elapsed().as_millis() as u64),
            };
            let _ = tx.send(result).await;
        });
        tasks.push(task);
    }

    let mut chapter_results = Vec::new();
    for _ in 0..total_chapters {
        if let Some(result) = rx.recv().await {
            chapter_results.push(result);
        }
    }

    chapter_results.sort_by_key(|r| r.index);
    let mut success_count = 0;
    let mut fail_count = 0;

    for result in &chapter_results {
        result.log();
        if result.success {
            let chapter = Chapter {
                title: result.title.clone(),
                content: result.content.clone(),
            };
            crawler.write_chapter(&chapter, result.index + 1)?;
            success_count += 1;
        } else {
            fail_count += 1;
        }
    }

    let total_duration = start_time.elapsed();
    let total_secs = total_duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    println!("{} =========================================", get_timestamp());
    println!("{} 爬取完成", get_timestamp());
    println!("{} 总章节: {} | 成功: {} | 失败: {}", get_timestamp(), total_chapters, success_count, fail_count);
    println!("{} 总耗时: {}h{}m{}s", get_timestamp(), hours, minutes, seconds);
    println!("{} 平均每章: {}ms", get_timestamp(), if success_count > 0 { total_duration.as_millis() as u64 / success_count as u64 } else { 0 });
    println!("{} 输出文件: {}", get_timestamp(), OUTPUT_FILE);
    println!("{} =========================================", get_timestamp());
    Ok(())
}