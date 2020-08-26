use std::path::Path;
use rusty_leveldb::{DB, Options};
use std::env::args;
use std::io::BufRead;
use reqwest::Url;
use bytes::{Bytes, Buf};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::ops::Add;


struct DownloadItem {
    href: String,
    is_html: bool,
}

struct ResourceItem<'a> {
    path: &'a str,
    content: &'a [u8],
}


fn main() {
    let epub_path = "epub";
    let mut cache_db = DB::open("cache.leveldb", Options::default())
        .expect("failed to open cache.leveldb");

    let default_files = vec![
        ResourceItem {
            path: "mimetype",
            content: b"application/epub+zip",
        },
        ResourceItem {
            path: "META-INF/container.xml",
            content: br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
   <rootfiles>
      <rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>
   </rootfiles>
</container>
"#,
        }
    ];

    if args().len() == 1 {
        // let mut url = String::new();
        let url = "http://reader.epubee.com/books/mobile/5f/5f80cfe69440056dc623f051c2f76246/";

        loop {
            // println!("please input url like: \
            // http://reader.epubee.com/books/mobile/5f/5f80cfe69440056dc623f051c2f76246/\n\
            // q to quit");
            //
            // std::io::stdin().read_line(&mut url).unwrap();
            //
            // if url.trim() == "q" {
            //     break;
            // }
            //
            // if url.len() != 74 {
            //     println!("invalid url format");
            //     continue;
            // };

            download_epub(url, &mut cache_db, &default_files);
            break;
        }
    }
}

fn download_epub(url: &str, mut db: &mut DB, default_files: &Vec<ResourceItem>) {
    let book_id = &url[41..73];

    let contents = cached_get(String::from(url).add("content.opf").as_str(), &mut db).unwrap();
    let contents_str = String::from_utf8(contents).unwrap();

    let mut title = String::new();
    let mut author = String::new();
    let mut download_list: Vec<DownloadItem> = Vec::new();

    parse_index(contents_str.as_str(), &mut title, &mut author, &mut download_list);


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
                    let attr = e.attributes().for_each(|a| {
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

fn cached_get(url: &str, db: &mut DB) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    return match db.get(url.as_bytes()) {
        Some(value) => Ok(value),
        None => {
            let bytes = reqwest::blocking::get(url)?
                .bytes()?;
            db.put(url.as_bytes(), bytes.bytes());
            Ok(bytes.to_vec())
        }
    };
}