use rand::seq::SliceRandom;
use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};
use serde::Deserialize;

const DEFAULT_CONCURRENT_LIMIT: usize = 15;
const DEFAULT_BASE_URL: &str = "https://www.alicesw.com/";
const DEFAULT_CATALOG_URL: &str = "https://www.alicesw.com/other/chapters/id/47686.html";
const DEFAULT_OUTPUT_FILE: &str = "output.txt";
const DEFAULT_TITLE_SELECTOR: &str = ".j_chapterName";
const DEFAULT_CONTENT_SELECTOR: &str = ".read-content p";
const DEFAULT_CHAPTER_LINK_SELECTOR: &str = ".mulu_list li a";

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    crawl: CrawlConfig,
    #[serde(default)]
    urls: UrlsConfig,
    #[serde(default)]
    selectors: SelectorsConfig,
    #[serde(default)]
    output: OutputConfig,
}

#[derive(Debug, Default, Deserialize)]
struct CrawlConfig {
    #[serde(default = "default_concurrent_limit")]
    concurrent_limit: usize,
}

#[derive(Debug, Default, Deserialize)]
struct UrlsConfig {
    #[serde(default = "default_base_url")]
    base_url: String,
    #[serde(default = "default_catalog_url")]
    catalog_url: String,
}

#[derive(Debug, Default, Deserialize)]
struct SelectorsConfig {
    #[serde(default = "default_title_selector")]
    title_selector: String,
    #[serde(default = "default_content_selector")]
    content_selector: String,
    #[serde(default = "default_chapter_link_selector")]
    chapter_link_selector: String,
}

#[derive(Debug, Default, Deserialize)]
struct OutputConfig {
    #[serde(default = "default_output_file")]
    file: String,
}

fn default_concurrent_limit() -> usize { DEFAULT_CONCURRENT_LIMIT }
fn default_base_url() -> String { DEFAULT_BASE_URL.to_string() }
fn default_catalog_url() -> String { DEFAULT_CATALOG_URL.to_string() }
fn default_title_selector() -> String { DEFAULT_TITLE_SELECTOR.to_string() }
fn default_content_selector() -> String { DEFAULT_CONTENT_SELECTOR.to_string() }
fn default_chapter_link_selector() -> String { DEFAULT_CHAPTER_LINK_SELECTOR.to_string() }
fn default_output_file() -> String { DEFAULT_OUTPUT_FILE.to_string() }

fn get_timestamp() -> String {
    let now = chrono::Local::now();
    now.format("[%H:%M:%S]").to_string()
}

fn find_config_file() -> Option<std::path::PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        let config_in_cwd = cwd.join("config.toml");
        if config_in_cwd.exists() {
            return Some(config_in_cwd);
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        let exe_dir = exe_path.parent().unwrap_or(&exe_path);
        let config_in_exe_dir = exe_dir.join("config.toml");
        if config_in_exe_dir.exists() {
            return Some(config_in_exe_dir);
        }
    }

    None
}

fn load_config() -> Config {
    let config_path = find_config_file();
    let config = match config_path {
        Some(ref path) => {
            println!("{} 已找到配置文件: {}", get_timestamp(), path.display());
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    match toml::from_str(&content) {
                        Ok(config) => config,
                        Err(e) => {
                            eprintln!("{} 配置文件解析失败，使用默认配置: {}", get_timestamp(), e);
                            Config::default()
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{} 无法读取配置文件，使用默认配置: {}", get_timestamp(), e);
                    Config::default()
                }
            }
        }
        None => {
            println!("{} 未找到 config.toml，使用默认配置", get_timestamp());
            Config::default()
        }
    };
    print_config(&config);
    config
}

fn print_config(config: &Config) {
    println!("{} =========================================", get_timestamp());
    println!("{} 当前配置:", get_timestamp());
    println!("{}   [crawl]", get_timestamp());
    println!("{}     concurrent_limit = {}", get_timestamp(), config.crawl.concurrent_limit);
    println!("{}   [urls]", get_timestamp());
    println!("{}     base_url = {}", get_timestamp(), config.urls.base_url);
    println!("{}     catalog_url = {}", get_timestamp(), config.urls.catalog_url);
    println!("{}   [selectors]", get_timestamp());
    println!("{}     title_selector = {}", get_timestamp(), config.selectors.title_selector);
    println!("{}     content_selector = {}", get_timestamp(), config.selectors.content_selector);
    println!("{}     chapter_link_selector = {}", get_timestamp(), config.selectors.chapter_link_selector);
    println!("{}   [output]", get_timestamp());
    println!("{}     file = {}", get_timestamp(), config.output.file);
    println!("{} =========================================", get_timestamp());
}

struct ChapterResult {
    index: usize,
    title: String,
    url: String,
    content: Vec<String>,
    success: bool,
    error_msg: Option<String>,
    duration_ms: u64,
    completed_at: chrono::DateTime<chrono::Local>,
}

impl ChapterResult {
    fn success(index: usize, title: String, url: String, content: Vec<String>, duration_ms: u64, completed_at: chrono::DateTime<chrono::Local>) -> Self {
        ChapterResult {
            index,
            title,
            url,
            content,
            success: true,
            error_msg: None,
            duration_ms,
            completed_at,
        }
    }

    fn failure(index: usize, url: String, error_msg: String, duration_ms: u64, completed_at: chrono::DateTime<chrono::Local>) -> Self {
        ChapterResult {
            index,
            title: String::new(),
            url,
            content: Vec::new(),
            success: false,
            error_msg: Some(error_msg),
            duration_ms,
            completed_at,
        }
    }

    fn log(&self) {
        let idx = self.index + 1;
        let timestamp = self.completed_at.format("[%H:%M:%S]").to_string();
        if self.success {
            println!("{} [{}] 爬取成功: {} ({}ms)", timestamp, idx, self.title, self.duration_ms);
        } else {
            println!("{} [{}] 爬取失败: {} ({})", timestamp, idx, self.url, self.error_msg.as_ref().unwrap_or(&String::new()));
        }
    }
}

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
    fn new(output_file: File, concurrent_limit: usize) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            semaphore: Arc::new(Semaphore::new(concurrent_limit)),
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

    let config = load_config();
    let concurrent_limit = config.crawl.concurrent_limit;
    let base_url = &config.urls.base_url;
    let catalog_url = &config.urls.catalog_url;
    let title_selector = &config.selectors.title_selector;
    let content_selector = &config.selectors.content_selector;
    let chapter_link_selector = &config.selectors.chapter_link_selector;
    let output_file_path = &config.output.file;

    let output_file = File::create(output_file_path)?;
    let mut crawler = Crawler::new(output_file, concurrent_limit)?;
    let client = reqwest::Client::new();
    let client_arc = Arc::new(client);

    println!("{} 开始获取章节列表...", get_timestamp());
    let catalog_start = Instant::now();
    let catalog_html = {
        let ua = USER_AGENTS.choose(&mut rand::thread_rng()).unwrap_or(&USER_AGENTS[0]);
        client_arc.get(catalog_url)
            .header("User-Agent", ua.to_string())
            .send()
            .await?.text().await?
    };
    let catalog_duration = catalog_start.elapsed().as_millis();
    let chapter_urls = {
        let document = scraper::Html::parse_document(&catalog_html);
        document.select(&scraper::Selector::parse(chapter_link_selector).unwrap())
            .filter_map(|a| a.value().attr("href"))
            .map(|href| {
                if href.starts_with("http") {
                    href.to_string()
                } else {
                    format!("{}{}", base_url, href.trim_start_matches('/'))
                }
            })
            .collect::<Vec<_>>()
    };
    let total_chapters = chapter_urls.len();
    println!("{} 章节列表获取成功，共 {} 章 ({}ms)", get_timestamp(), total_chapters, catalog_duration);
    println!("{} 开始并发爬取（并发数: {}）", get_timestamp(), concurrent_limit);

    let chapter_urls_arc = Arc::new(chapter_urls);
    let semaphore_arc = crawler.semaphore.clone();
    let title_sel = scraper::Selector::parse(title_selector).unwrap();
    let content_sel = scraper::Selector::parse(content_selector).unwrap();
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
            let _permit = semaphore.acquire().await.unwrap();
            let fetch_start = Instant::now();
            let completed_at = chrono::Local::now();
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
                                ChapterResult::success(index, chapter_title, url, paragraphs, fetch_start.elapsed().as_millis() as u64, completed_at)
                            }
                            None => ChapterResult::failure(index, url, "Chapter title not found".to_string(), fetch_start.elapsed().as_millis() as u64, completed_at),
                        }
                    }
                    Err(e) => ChapterResult::failure(index, url, format!("Request failed: {}", e), fetch_start.elapsed().as_millis() as u64, completed_at),
                },
                Err(e) => ChapterResult::failure(index, url, format!("Send failed: {}", e), fetch_start.elapsed().as_millis() as u64, completed_at),
            };
            let _ = tx.send(result).await;
        });
        tasks.push(task);
    }

    let mut chapter_results = Vec::new();
    let mut pending_count = total_chapters;
    let mut success_count = 0;
    let mut fail_count = 0;

    println!("{} 等待爬取结果...", get_timestamp());
    let mut waiting_time = 0;
    while pending_count > 0 {
        match timeout(Duration::from_secs(30), rx.recv()).await {
            Ok(Some(result)) => {
                result.log();
                chapter_results.push(result);
                pending_count -= 1;
                waiting_time = 0;
                if pending_count % 100 == 0 && pending_count > 0 {
                    println!("{} 剩余 {} 章待处理...", get_timestamp(), pending_count);
                }
            }
            Ok(None) => {
                println!("{} 通道已关闭，但还有 {} 章未完成", get_timestamp(), pending_count);
                break;
            }
            Err(_) => {
                waiting_time += 30;
                println!("{} 等待超时 ({}s)，剩余 {} 章...", get_timestamp(), waiting_time, pending_count);
                if waiting_time > 300 {
                    println!("{} 等待时间过长，放弃等待未完成的章节", get_timestamp());
                    break;
                }
            }
        }
    }
    println!("{} 所有结果已接收 (共 {} 章)，开始写入文件...", get_timestamp(), chapter_results.len());

    chapter_results.sort_by_key(|r| r.index);
    let write_start = Instant::now();
    println!("{} 开始写入 {} 章到文件...", get_timestamp(), chapter_results.len());

    for (i, result) in chapter_results.iter().enumerate() {
        if result.success {
            let chapter = Chapter {
                title: result.title.clone(),
                content: result.content.clone(),
            };
            match crawler.write_chapter(&chapter, result.index + 1) {
                Ok(_) => success_count += 1,
                Err(e) => {
                    eprintln!("{} 第{}章写入失败: {}", get_timestamp(), result.index + 1, e);
                    fail_count += 1;
                }
            }
        } else {
            fail_count += 1;
        }
        if (i + 1) % 100 == 0 {
            println!("{} 已写入 {}/{} 章...", get_timestamp(), i + 1, chapter_results.len());
        }
    }
    let write_duration = write_start.elapsed().as_millis();
    println!("{} 文件写入完成 ({}ms)", get_timestamp(), write_duration);

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
    println!("{} 输出文件: {}", get_timestamp(), output_file_path);
    println!("{} =========================================", get_timestamp());
    Ok(())
}
