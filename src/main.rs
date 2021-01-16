use std::env::args;
use std::fs::File;
use std::io::Write;
use std::ops::Add;
use std::path::Path;
use std::time::Duration;

use bytes::Buf;
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::{Result, Url};
use reqwest::blocking::Client;
use rusty_leveldb::{DB, Options};
use zip::write::FileOptions;

struct DownloadItem {
    href: String,
    is_html: bool,
}

struct ResourceItem {
    path: String,
    content: Vec<u8>,
}


fn main() {
    let download_path = Path::new("epub");
    if !download_path.exists() {
        std::fs::create_dir(&download_path).unwrap();
    };

    let mut cache_db = DB::open("cache.leveldb", Options::default())
        .expect("failed to open cache.leveldb");

    let default_files = vec![
        ResourceItem {
            path: String::from("mimetype"),
            content: b"application/epub+zip".to_vec(),
        },
        ResourceItem {
            path: String::from("META-INF/container.xml"),
            content: br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
   <rootfiles>
      <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
   </rootfiles>
</container>
"#.to_vec(),
        }
    ];

    if args().len() == 1 {
        let mut url = String::new();
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build().unwrap();

        loop {
            println!("please input url like: \
            http://reader.epubee.com/books/mobile/5f/5f80cfe69440056dc623f051c2f76246/\n\
            q to quit");

            url.clear();
            std::io::stdin().read_line(&mut url).unwrap();
            // remove trailing \n or \r\n
            url.pop().unwrap();
            if url.ends_with("\r") {
                url.pop().unwrap();
            }

            if url == "q" {
                break;
            }

            if url.len() != 74 {
                println!("invalid url format");
                continue;
            };

            download_epub(&url, &mut cache_db, &download_path, &default_files, &client);
        }
    }
}

fn download_epub(url: &str, mut db: &mut DB, download_path: &Path,
                 default_files: &Vec<ResourceItem>, client: &Client) {
    let contents = cached_get(String::from(url).add("content.opf").as_str(),
                              &mut db, &client).unwrap();
    let contents_str = String::from_utf8(contents).unwrap();

    let mut title = String::new();
    let mut author = String::new();
    let mut download_list: Vec<DownloadItem> = Vec::new();

    parse_index(contents_str.as_str(), &mut title, &mut author, &mut download_list);

    let mut resource_list: Vec<ResourceItem> = Vec::new();

    let base_url = Url::parse(url).unwrap();
    for download_item in download_list {
        let url = base_url.join(download_item.href.as_str()).unwrap();
        let content = cached_get(url.as_str(), &mut db, &client).unwrap();
        if download_item.is_html {
            let new_html = parse_content(content);

            resource_list.push(ResourceItem {
                path: String::from(download_item.href),
                content: new_html,
            })
        } else {
            resource_list.push(ResourceItem { path: String::from(download_item.href), content })
        };
    }

    let epub_path = download_path.join(
        Path::new(format!("{} - {}.epub", title, author)
            .replace(":", " ")
            .replace("/", " ")
            .replace("\\", " ")
            .replace("*", " ")
            .replace("\"", " ")
            .replace("<", " ")
            .replace(">", " ")
            .replace("|", " ")
            .as_str()));
    if epub_path.exists() {
        std::fs::remove_file(&epub_path).unwrap();
    };
    let mut epub = zip::ZipWriter::new(File::create(&epub_path).unwrap());
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    epub.start_file("content.opf", options).unwrap();
    epub.write_all(&contents_str.into_bytes()).unwrap();

    for item in default_files {
        epub.start_file(&item.path, options).unwrap();
        epub.write_all(&item.content).unwrap();
    }

    for item in resource_list {
        epub.start_file(&item.path, options).unwrap();
        epub.write_all(&item.content).unwrap();
    }

    epub.finish().unwrap();

    println!("epub saved in {}", epub_path.to_str().unwrap());
}

fn parse_content(content: Vec<u8>) -> Vec<u8> {
    let html = String::from_utf8(content).unwrap();

    let head_prefix = "<head>";
    let head_postfix = "<script";
    let head_start = html.find(head_prefix).unwrap() + head_prefix.len();
    let head_end = match html.find(head_postfix) {
        Some(x) => {
            x - 1
        }
        None => {
            return html.as_bytes().to_vec();
        }
    };

    let content_prefix = r#"<div class="readercontent"><div class="readercontent-inner">"#;
    let content_postfix = r#"</div></div></div>"#;
    let content_start = html.find(content_prefix).unwrap() + content_prefix.len();
    let content_end = html.rfind(content_postfix).unwrap() - 1;

    let new_html = format!(r#"<?xml version='1.0' encoding='utf-8'?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head>
    {}
  </head>
  <body class="calibre">
    {}
  </body>
</html>"#,
                           &html.as_str()[head_start..head_end],
                           &html.as_str()[content_start..content_end]
    ).as_bytes().to_vec();
    new_html
}

fn parse_index(contents_str: &str, title: &mut String, author: &mut String, download_list: &mut Vec<DownloadItem>) {
    let mut is_title = false;
    let mut is_author = false;
    let mut is_manifest = false;

    let mut reader = Reader::from_str(contents_str);
    let mut buf = Vec::new();
    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name() {
                    b"dc:title" => {
                        is_title = true;
                    }
                    b"dc:creator" => {
                        is_author = true;
                    }
                    b"manifest" => {
                        is_manifest = true;
                    }
                    _ => {}
                };
            }
            Ok(Event::End(ref e)) => {
                if e.name() == b"manifest" {
                    is_manifest = false;
                }
            }
            Ok(Event::Empty(ref e)) => {
                if is_manifest && e.name() == b"item" {
                    let mut item: DownloadItem = DownloadItem { href: "".to_string(), is_html: false };
                    e.attributes().for_each(|a| {
                        let a = a.unwrap();
                        match a.key {
                            b"href" => {
                                item.href = String::from_utf8(a.value.to_vec()).unwrap();
                            }
                            b"media-type" => {
                                item.is_html = a.value.to_vec().eq(b"application/xhtml+xml");
                            }
                            _ => ()
                        }
                    });
                    download_list.push(item);
                }
            }
            Ok(Event::Text(e)) => {
                if is_title {
                    *title = e.unescape_and_decode(&reader).unwrap();
                    is_title = false;
                };
                if is_author {
                    *author = e.unescape_and_decode(&reader).unwrap();
                    is_author = false;
                };
            }
            Ok(Event::Eof) => break,
            _ => {}
        };
    }
}

fn cached_get(url: &str, db: &mut DB, client: &Client) -> Result<Vec<u8>> {
    return match db.get(url.as_bytes()) {
        Some(value) => {
            println!("cache hit {}", url);
            Ok(value)
        }
        None => {
            let mut buf = String::new();
            let response = loop {
                match client.get(url).send() {
                    Ok(x) => break x,
                    Err(e) => {
                        if e.is_timeout() {
                            continue;
                        }
                        println!("failed to download, please check network.\npress enter to continue.");
                        std::io::stdin().read_line(&mut buf).unwrap();
                    }
                };
            };
            let bytes = response
                .bytes()?;
            db.put(url.as_bytes(), bytes.bytes()).unwrap();
            println!("downloaded {}", url);
            Ok(bytes.to_vec())
        }
    };
}