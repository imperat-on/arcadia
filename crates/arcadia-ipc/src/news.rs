//! Notícias de jogos por RSS — port de `news.js`.
//!
//! Fetch de 6 feeds, parse por regex (mesmo estilo do cliente Node), filtro
//! anti-oferta/hardware e cache de 30 minutos. Testes usam fixtures; a rede
//! real só é tocada por `get_news`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

const UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36";
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

const FEEDS: [(&str, &str); 6] = [
    ("Eurogamer", "https://www.eurogamer.pt/feed"),
    (
        "Nintendo Blast",
        "https://www.nintendoblast.com.br/feeds/posts/default?alt=rss",
    ),
    ("GameSpot", "https://www.gamespot.com/feeds/game-news/"),
    ("Rock Paper Shotgun", "https://www.rockpapershotgun.com/feed"),
    ("Push Square", "https://www.pushsquare.com/feeds/latest"),
    ("PC Gamer", "https://www.pcgamer.com/rss/"),
];

pub struct NewsCache {
    cached: Mutex<Option<(Instant, Vec<Value>)>>,
}

impl Default for NewsCache {
    fn default() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }
}

impl NewsCache {
    pub async fn get(&self, limit: usize) -> Vec<Value> {
        if let Ok(guard) = self.cached.lock() {
            if let Some((stamp, items)) = guard.as_ref() {
                if stamp.elapsed() < CACHE_TTL {
                    return items.iter().take(limit).cloned().collect();
                }
            }
        }
        let items = fetch_all(limit).await;
        if !items.is_empty() {
            if let Ok(mut guard) = self.cached.lock() {
                *guard = Some((Instant::now(), items.clone()));
            }
        }
        items
    }
}

fn decode_numeric(text: &str) -> String {
    let hex = Regex::new(r"&#x([0-9a-fA-F]+);").expect("hex entity regex");
    let dec = Regex::new(r"&#(\d+);").expect("dec entity regex");
    let text = hex
        .replace_all(text, |caps: &regex::Captures| {
            u32::from_str_radix(&caps[1], 16)
                .ok()
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_default()
        })
        .into_owned();
    dec.replace_all(&text, |caps: &regex::Captures| {
        caps[1]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|c| c.to_string())
            .unwrap_or_default()
    })
    .into_owned()
}

pub fn clean_text(value: &str) -> String {
    let mut text = decode_numeric(value)
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");
    let tags = Regex::new(r"<[^>]+>").expect("tag regex");
    text = tags.replace_all(&text, " ").into_owned();
    text = text.replace("&amp;", "&");
    let spaces = Regex::new(r"\s+").expect("space regex");
    spaces.replace_all(&text, " ").trim().to_string()
}

fn pick(block: &str, tag: &str) -> String {
    let pattern = format!(r"(?is)<{tag}[^>]*>([\s\S]*?)</{tag}>");
    let Ok(regex) = Regex::new(&pattern) else {
        return String::new();
    };
    let Some(captures) = regex.captures(block) else {
        return String::new();
    };
    captures[1]
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>")
        .trim()
        .to_string()
}

fn improve_image(url: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    let mut url = url.replace("&amp;", "&");
    if url.contains("blogger.googleusercontent.com") || url.contains("bp.blogspot.com") {
        let regex = Regex::new(r"(?i)/(s\d+(?:-[a-z]\d+)*(?:-c)?)/").expect("blogger regex");
        return regex.replace(&url, "/s0/").into_owned();
    }
    if url.contains("images.pushsquare.com") {
        let regex = Regex::new(r"(?i)/(small|medium|thumb)\.(jpg|png|webp)").expect("push regex");
        return regex.replace(&url, "/large.$2").into_owned();
    }
    if url.contains("gamespot.com") {
        let regex = Regex::new(r"\?w=\d+").expect("gamespot regex");
        if url.contains("?w=") {
            url = regex.replace(&url, "?w=1280").into_owned();
        }
        return url;
    }
    if url.contains("gnwcdn.com") {
        let width = Regex::new(r"([?&]width=)\d+").expect("width regex");
        let quality = Regex::new(r"([?&]quality=)\d+").expect("quality regex");
        url = width.replace(&url, "${1}1280").into_owned();
        url = quality.replace(&url, "${1}90").into_owned();
        return url;
    }
    url
}

fn pick_image(block: &str) -> String {
    let attr = Regex::new(r#"(?is)<(?:media:content|media:thumbnail|enclosure)[^>]*\burl="([^"]+)""#)
        .expect("media regex");
    if let Some(captures) = attr.captures(block) {
        return improve_image(&captures[1]);
    }
    let img = Regex::new(r#"(?is)<img[^>]*\bsrc="([^"]+)""#).expect("img regex");
    if let Some(captures) = img.captures(block) {
        return improve_image(&captures[1]);
    }
    let any = Regex::new(r#"(?i)https?://[^"'\s<>]+\.(?:jpg|jpeg|png|webp)"#).expect("url regex");
    any.find(block)
        .map(|found| improve_image(found.as_str()))
        .unwrap_or_default()
}

fn base36(mut value: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while value > 0 {
        out.push(digits[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "0".to_string())
}

fn hash_id(link: &str) -> String {
    let mut hash: i32 = 5381;
    for unit in link.encode_utf16() {
        hash = hash.wrapping_shl(5).wrapping_add(hash).wrapping_add(unit as i32);
    }
    format!("news_{}", base36(hash as u32))
}

fn is_game(item: &Value) -> bool {
    let title = item.get("title").and_then(Value::as_str).unwrap_or("");
    let url = item.get("url").and_then(Value::as_str).unwrap_or("");
    let block_url = Regex::new(r"(?i)/(descontos|ofertas|deals|promo(?:cao|coes)?|hardware|reviews/hardware|perifericos|celular|smartphone)\b").expect("block url");
    let block_offer = Regex::new(r"(?i)(cai de pre|menor pre|melhor pre|mais barat|desconto|em oferta|black friday|cupom|\bdeal\b|% off|por (?:apenas|cerca de|r\$))").expect("block offer");
    let hw = r"(?i)\b(gpu|cpu|ssd|hd externo|placa de v[íi]deo|placa-m[ãa]e|monitor|headset|fone|teclado|gabinete|cooler|fonte de alimenta|notebook|smartphone|celular|processador|cadeira gamer|power ?bank|roteador|smart ?tv|geladeira|carregador|rtx|gtx|geforce|radeon|ryzen|intel)(?:es|s)?\b";
    let block_hw = Regex::new(hw).expect("block hw");
    let device = r"(?i)(steam deck|rog ally|legion go|legion c\d|msi claw|\bhandheld\b|console port[áa]til|ayn odin|retroid|samsung galaxy|galaxy s\d|galaxy z|iphone|ipad|macbook|pixel \d)";
    let block_device = Regex::new(device).expect("block device");
    !(block_url.is_match(url)
        || block_offer.is_match(title)
        || block_hw.is_match(title)
        || block_device.is_match(title))
}

fn parse_feed(source: &str, xml: &str) -> Vec<Value> {
    let items_re = Regex::new(r"(?is)<item[\s>][\s\S]*?</item>").expect("item regex");
    let date_re = Regex::new(r"(?i)([A-Z][a-z]{2}, \d{1,2} [A-Z][a-z]{2} \d{4} \d{2}:\d{2}:\d{2} [+-]\d{4})")
        .expect("date regex");
    items_re
        .find_iter(xml)
        .filter_map(|block| {
            let block = block.as_str();
            let link = {
                let link = pick(block, "link");
                if link.is_empty() {
                    pick(block, "guid")
                } else {
                    link
                }
            };
            let title = clean_text(&pick(block, "title"));
            if title.is_empty() || link.is_empty() {
                return None;
            }
            let summary: String = clean_text(&pick(block, "description"))
                .chars()
                .take(280)
                .collect();
            let raw_date = {
                let pub_date = pick(block, "pubDate");
                if pub_date.is_empty() {
                    pick(block, "dc:date")
                } else {
                    pub_date
                }
            };
            let date = date_re
                .find(&raw_date)
                .and_then(|found| chrono::DateTime::parse_from_rfc2822(found.as_str()).ok())
                .map(|date| date.with_timezone(&chrono::Utc).to_rfc3339())
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc3339(raw_date.trim())
                        .ok()
                        .map(|date| date.with_timezone(&chrono::Utc).to_rfc3339())
                })
                .unwrap_or_default();
            Some(json!({
                "id": hash_id(&link),
                "title": title,
                "summary": summary,
                "source": source,
                "url": link,
                "image": pick_image(block),
                "date": date
            }))
        })
        .collect()
}

/// Junta feeds, remove duplicados/ofertas, ordena e corta.
pub fn merge_feeds(feeds: &[(&str, &[Value])], limit: usize) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    let mut all: Vec<Value> = Vec::new();
    for (_source, items) in feeds {
        for item in *items {
            let url = item.get("url").and_then(Value::as_str).unwrap_or("");
            if url.is_empty() || seen.contains(url) || !is_game(item) {
                continue;
            }
            seen.insert(url.to_string());
            all.push(item.clone());
        }
    }
    all.sort_by(|left, right| {
        let left = left.get("date").and_then(Value::as_str).unwrap_or("");
        let right = right.get("date").and_then(Value::as_str).unwrap_or("");
        right.cmp(left)
    });
    all.truncate(limit);
    all
}

async fn fetch_feed(client: &reqwest::Client, source: &str, url: &str) -> Vec<Value> {
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, UA)
        .timeout(Duration::from_secs(8))
        .send()
        .await;
    let Ok(response) = response else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(xml) = response.text().await else {
        return Vec::new();
    };
    parse_feed(source, &xml)
}

async fn fetch_all(limit: usize) -> Vec<Value> {
    let Ok(client) = reqwest::Client::builder().build() else {
        return Vec::new();
    };
    let mut handles = Vec::new();
    for (source, url) in FEEDS {
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            fetch_feed(&client, source, url).await
        }));
    }
    let mut collected: Vec<(&str, Vec<Value>)> = Vec::new();
    for (handle, (source, _)) in handles.into_iter().zip(FEEDS) {
        collected.push((source, handle.await.unwrap_or_default()));
    }
    let refs: Vec<(&str, &[Value])> = collected
        .iter()
        .map(|(source, items)| (*source, items.as_slice()))
        .collect();
    merge_feeds(&refs, limit)
}

/// `news:game` ainda não portado (notícias por appid vêm do Steam news).
pub fn game_news(_appid: &str) -> Value {
    json!([])
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<rss><channel>
      <item><title>Novo jogo de plataforma chega em março</title><link>https://site/game</link>
        <description>&lt;p&gt;Um &lt;b&gt;jogo&lt;/b&gt; novo.&lt;/p&gt;</description>
        <pubDate>Mon, 09 Nov 2026 10:00:00 +0000</pubDate>
        <media:content url="https://cdn/site/image.jpg" /></item>
      <item><title>RTX 5090 em oferta com desconto</title><link>https://site/hardware/gpu</link>
        <pubDate>Mon, 09 Nov 2026 09:00:00 +0000</pubDate></item>
      <item><title>Review de jogo indie</title><link>https://site/indie</link>
        <pubDate>Mon, 09 Nov 2026 11:00:00 +0000</pubDate></item>
    </channel></rss>"#;

    #[test]
    fn parses_cleans_and_keeps_only_game_news() {
        let items = parse_feed("Site", FEED);
        assert_eq!(items.len(), 3);
        let merged = merge_feeds(&[("Site", &items)], 10);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0]["title"], "Review de jogo indie");
        assert!(merged.iter().all(|item| !item["title"]
            .as_str()
            .unwrap()
            .contains("RTX")));
        let game = merged
            .iter()
            .find(|item| item["url"] == "https://site/game")
            .unwrap();
        assert_eq!(game["summary"], "Um jogo novo.");
        assert_eq!(game["image"], "https://cdn/site/image.jpg");
        assert!(game["date"].as_str().unwrap().starts_with("2026-11-09"));
    }

    #[test]
    fn improves_known_cdn_images() {
        assert_eq!(
            improve_image("https://images.pushsquare.com/small.jpg"),
            "https://images.pushsquare.com/large.jpg"
        );
        assert_eq!(
            improve_image("https://www.gamespot.com/a.jpg?w=300"),
            "https://www.gamespot.com/a.jpg?w=1280"
        );
        assert_eq!(
            improve_image("https://cdn.gnwcdn.com/a.jpg?width=690&quality=80"),
            "https://cdn.gnwcdn.com/a.jpg?width=1280&quality=90"
        );
    }

    #[test]
    fn hash_is_stable_and_short() {
        assert_eq!(hash_id("https://a"), hash_id("https://a"));
        assert!(hash_id("https://a").starts_with("news_"));
        assert_ne!(hash_id("https://a"), hash_id("https://b"));
    }
}
