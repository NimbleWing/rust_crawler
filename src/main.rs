use std::fs::File;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp = reqwest::Client::new()
        .get("https://www.alicesw.com/book/49017/b914f17bebada.html")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3")
        .send()
        .await?
        .text()
        .await?;
    let document = scraper::Html::parse_document(&resp);
    let selector = scraper::Selector::parse(r#".j_chapterName"#).unwrap();
    let title = document.select(&selector).next().unwrap();
    let chapter_title = title.text().collect::<Vec<_>>().join("");
    println!("{}", chapter_title);
    let content_selector = scraper::Selector::parse(r#".read-content p"#).unwrap();
    let next_link_selector = scraper::Selector::parse(r#"#j_chapterNext"#).unwrap();
    let mut file = File::create("output.txt")?;
    let mut current_url = "https://www.alicesw.com/book/49017/b914f17bebada.html".to_string();
    let base_url = "https://www.alicesw.com".to_string();
    let mut chapter_count = 0;
    loop {
        let resp = reqwest::Client::new()
            .get(&current_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/58.0.3029.110 Safari/537.3")
            .send()
            .await?
            .text()
            .await?;
        let document = scraper::Html::parse_document(&resp);
        let title = document.select(&selector).next().unwrap();
        let chapter_title = title.text().collect::<Vec<_>>().join("");
        println!("第{}章: {}", chapter_count + 1, chapter_title);
        let mut output = String::new();
        output.push_str(&chapter_title);
        output.push('\n');
        let paragraphs: Vec<_> = document.select(&content_selector).collect();
        for paragraph in paragraphs {
            let text = paragraph.text().collect::<Vec<_>>().join("");
            if !text.is_empty() {
                output.push_str(&text);
                output.push('\n');
            }
        }
        file.write_all(output.as_bytes())?;
        chapter_count += 1;
        let next_link = document.select(&next_link_selector).next();
        match next_link {
            Some(next) => {
                if let Some(href) = next.value().attr("href") {
                    if href.starts_with("http") {
                        current_url = href.to_string();
                    } else {
                        current_url = base_url.clone() + href;
                    }
                } else {
                    break;
                }
            }
            None => {
                break;
            }
        }
        // tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    println!("共爬取{}章内容，已写入output.txt", chapter_count);
    Ok(())
}