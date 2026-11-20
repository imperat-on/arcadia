//! Conversor tolerante do HTML da loja para blocos de texto rico.
//!
//! O iced não executa HTML. Este módulo transforma a descrição (`about` do
//! `game:sysinfo` ou a descrição já limpa da biblioteca) em parágrafos com
//! spans de negrito e links. Nenhuma tag vira widget e nenhum script/style é
//! interpretado: o conteúdo é convertido em texto antes de chegar à UI.
//!
//! Limitação desta onda: links não abrem o navegador. Para não perder a
//! referência, o parser anexa a URL ao rótulo (`texto (https://…)`), que fica
//! visível e copiável no painel de descrição.

/// Trecho de texto com a formatação que o painel de descrição conhece.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichSpan {
    pub text: String,
    pub bold: bool,
    /// O trecho pertence a um link; a URL também aparece como texto.
    pub link: bool,
}

/// Parágrafo (ou item de lista) da descrição.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RichBlock {
    pub spans: Vec<RichSpan>,
}

impl RichBlock {
    fn push(&mut self, text: &str, bold: bool, link: bool) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut() {
            if last.bold == bold && last.link == link {
                last.text.push_str(text);
                return;
            }
        }
        self.spans.push(RichSpan {
            text: text.to_string(),
            bold,
            link,
        });
    }

    /// Texto plano do bloco; usado nos testes e por consumidores simples.
    #[cfg(test)]
    pub fn plain_text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }

    fn has_content(&self) -> bool {
        self.spans.iter().any(|span| !span.text.trim().is_empty())
    }

    fn ends_with_space_or_break(&self) -> bool {
        matches!(
            self.spans.last().and_then(|span| span.text.chars().last()),
            Some(' ' | '\n')
        )
    }

    fn trim_end(&mut self) {
        while let Some(last) = self.spans.last_mut() {
            let trimmed = last.text.trim_end_matches([' ', '\n']).to_string();
            last.text = trimmed;
            if last.text.is_empty() {
                self.spans.pop();
            } else {
                break;
            }
        }
    }
}

/// Converte HTML (ou texto simples) em parágrafos renderizáveis.
///
/// Regras principais:
/// - `p`, `div`, `li`, títulos e quebras de bloco separam parágrafos;
/// - `br` mantém a quebra dentro do parágrafo;
/// - `b`/`strong` (e títulos) produzem spans em negrito;
/// - `a href` vira texto + ` (url)` com o esquema perigoso descartado;
/// - `script`, `style`, `iframe`, `object`, `embed`, `svg` e `math` são
///   removidos junto do conteúdo;
/// - entidades HTML comuns e numéricas são decodificadas.
pub fn parse_description(html: &str) -> Vec<RichBlock> {
    let mut parser = Parser::default();
    parser.run(html);
    parser.finish()
}

#[derive(Default)]
struct Parser {
    blocks: Vec<RichBlock>,
    current: RichBlock,
    bold_depth: u32,
    heading_bold: bool,
    link: Option<LinkState>,
    pending_bullet: bool,
    pending_inline_space: bool,
}

struct LinkState {
    href: String,
    span_start: usize,
}

impl Parser {
    fn run(&mut self, input: &str) {
        let mut index = 0;
        while index < input.len() {
            let Some(offset) = input[index..].find('<') else {
                self.push_text(&input[index..]);
                break;
            };
            let start = index + offset;
            self.push_text(&input[index..start]);

            if input[start..].starts_with("<!--") {
                match input[start + 4..].find("-->") {
                    Some(end) => {
                        index = start + 4 + end + 3;
                        continue;
                    }
                    None => break,
                }
            }

            let Some(tag) = parse_tag(&input[start..]) else {
                // `<` literal (ex.: "1 < 2"): não é uma tag.
                self.push_text("<");
                index = start + 1;
                continue;
            };

            if !tag.close && is_skipped_element(&tag.name) {
                let closing = format!("</{}", tag.name);
                let lower = input[start..].to_ascii_lowercase();
                if let Some(position) = lower.find(&closing) {
                    let after = start + position;
                    match input[after..].find('>') {
                        Some(end) => {
                            index = after + end + 1;
                            continue;
                        }
                        None => break,
                    }
                }
                // Elemento de execução sem fechamento: descarta o resto.
                break;
            }

            self.handle_tag(&tag);
            index = start + tag.end;
        }
        self.finalize_block();
    }

    fn finish(mut self) -> Vec<RichBlock> {
        self.close_link();
        self.finalize_block();
        self.blocks
    }

    fn handle_tag(&mut self, tag: &Tag) {
        match tag.name.as_str() {
            "br" => self.line_break(),
            "p" | "div" | "blockquote" | "section" | "article" | "header" | "footer" | "table"
            | "thead" | "tbody" | "tr" | "td" | "th" => {
                self.finalize_block();
            }
            "li" => {
                self.finalize_block();
                if tag.close {
                    self.pending_bullet = false;
                } else {
                    self.pending_bullet = true;
                }
            }
            "ul" | "ol" => {
                self.finalize_block();
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.finalize_block();
                self.heading_bold = !tag.close;
            }
            "b" | "strong" => {
                if tag.close {
                    self.bold_depth = self.bold_depth.saturating_sub(1);
                } else if !tag.self_closing {
                    self.bold_depth = self.bold_depth.saturating_add(1);
                }
            }
            "a" => {
                if tag.close {
                    self.close_link();
                } else {
                    // Um novo <a> sem fechamento anterior não pode misturar
                    // links: fecha o anterior antes de abrir.
                    self.close_link();
                    if let Some(href) = href_from_attrs(&tag.attrs) {
                        self.link = Some(LinkState {
                            href,
                            span_start: self.current.spans.len(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn line_break(&mut self) {
        if !self.current.has_content() {
            return;
        }
        if !self.current.ends_with_space_or_break() {
            self.current.push("\n", false, false);
        }
    }

    fn push_text(&mut self, raw: &str) {
        let decoded = decode_entities(raw);
        let leading_space = decoded.chars().next().is_some_and(char::is_whitespace);
        let mut normalized = String::with_capacity(decoded.len());
        let mut space_pending = false;

        for character in decoded.chars() {
            if character.is_whitespace() {
                space_pending = true;
                continue;
            }
            if character.is_control() {
                continue;
            }
            if space_pending && !normalized.is_empty() {
                normalized.push(' ');
            }
            space_pending = false;
            normalized.push(character);
        }
        if space_pending && !normalized.is_empty() {
            normalized.push(' ');
        }

        if normalized.trim().is_empty() {
            if self.current.has_content() && !self.current.ends_with_space_or_break() {
                self.pending_inline_space = true;
            }
            return;
        }

        if !self.current.has_content() || self.current.ends_with_space_or_break() {
            normalized = normalized.trim_start().to_string();
        } else if (self.pending_inline_space || leading_space) && !normalized.starts_with(' ') {
            normalized.insert(0, ' ');
        }
        self.pending_inline_space = false;

        if normalized.is_empty() {
            return;
        }

        if self.pending_bullet && !self.current.has_content() {
            self.current.push("• ", false, false);
            self.pending_bullet = false;
        }

        let bold = self.bold_depth > 0 || self.heading_bold;
        let link = self.link.is_some();
        self.current.push(&normalized, bold, link);
    }

    fn close_link(&mut self) {
        let Some(link) = self.link.take() else {
            return;
        };
        let label: String = self
            .current
            .spans
            .get(link.span_start..)
            .unwrap_or_default()
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        let label = label.trim();

        if label.is_empty() {
            self.current.push(&link.href, false, true);
            return;
        }
        if !label.contains(link.href.as_str()) {
            self.current.push(" (", false, true);
            self.current.push(&link.href, false, true);
            self.current.push(")", false, true);
        }
    }

    fn finalize_block(&mut self) {
        self.close_link();
        self.current.trim_end();
        if self.current.has_content() {
            self.blocks.push(std::mem::take(&mut self.current));
        } else {
            self.current = RichBlock::default();
        }
        self.pending_inline_space = false;
    }
}

#[derive(Debug)]
struct Tag {
    name: String,
    close: bool,
    self_closing: bool,
    /// Bytes consumidos, incluindo o `>`.
    end: usize,
    /// Atributos crus entre o nome e o fechamento da tag.
    attrs: String,
}

fn parse_tag(input: &str) -> Option<Tag> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }

    let mut cursor = 1;
    if bytes.get(cursor) == Some(&b'!') || bytes.get(cursor) == Some(&b'?') {
        let end = input.find('>')?;
        return Some(Tag {
            name: String::new(),
            close: false,
            self_closing: true,
            end: end + 1,
            attrs: String::new(),
        });
    }

    let close = bytes.get(cursor) == Some(&b'/');
    if close {
        cursor += 1;
    }

    let name_start = cursor;
    while let Some(byte) = bytes.get(cursor) {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':' | b'_') {
            cursor += 1;
        } else {
            break;
        }
    }
    if cursor == name_start {
        return None;
    }

    let name = input[name_start..cursor].to_ascii_lowercase();
    let attrs_start = cursor;
    let mut quote: Option<u8> = None;
    while let Some(byte) = bytes.get(cursor) {
        match quote {
            Some(quoted) if *byte == quoted => quote = None,
            Some(_) => {}
            None if matches!(byte, b'"' | b'\'') => quote = Some(*byte),
            None if *byte == b'>' => {
                let attrs = input[attrs_start..cursor].to_string();
                let self_closing = attrs.trim_end().ends_with('/');
                return Some(Tag {
                    name,
                    close,
                    self_closing,
                    end: cursor + 1,
                    attrs,
                });
            }
            None => {}
        }
        cursor += 1;
    }

    None
}

fn is_skipped_element(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "iframe" | "object" | "embed" | "svg" | "math"
    )
}

fn href_from_attrs(attrs: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let position = lower.find("href")?;
    let mut rest = &attrs[position + 4..];
    rest = rest.trim_start();
    rest = rest.strip_prefix('=')?.trim_start();

    let value = if let Some(stripped) = rest.strip_prefix('"') {
        &stripped[..stripped.find('"')?]
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        &stripped[..stripped.find('\'')?]
    } else {
        rest.split_whitespace().next()?
    };

    let value = decode_entities(value);
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    let lower = value.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        return None;
    }
    if let Some(rest) = value.strip_prefix("//") {
        return Some(format!("https:{rest}"));
    }
    Some(value.to_string())
}

fn decode_entities(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        let after = &rest[position + 1..];
        let Some(end) = after.find(';').filter(|end| *end <= 12) else {
            output.push('&');
            rest = after;
            continue;
        };
        let entity = &after[..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "hellip" => Some('…'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "trade" => Some('™'),
            _ => entity
                .strip_prefix('#')
                .and_then(|number| {
                    if let Some(hex) = number
                        .strip_prefix('x')
                        .or_else(|| number.strip_prefix('X'))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        number.parse::<u32>().ok()
                    }
                })
                .and_then(char::from_u32),
        };

        match decoded {
            Some(character) => {
                output.push(character);
                rest = &after[end + 1..];
            }
            None => {
                output.push('&');
                rest = after;
            }
        }
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(blocks: &[RichBlock]) -> Vec<String> {
        blocks.iter().map(RichBlock::plain_text).collect()
    }

    #[test]
    fn parses_paragraphs_breaks_bold_and_entities() {
        let blocks = parse_description(
            "<p>Olá <b>mundo</b> &amp; cia</p><p>Linha 1<br/>Linha 2 &#65;&#x42;</p>",
        );

        assert_eq!(
            plain(&blocks),
            vec!["Olá mundo & cia", "Linha 1\nLinha 2 AB"]
        );
        assert!(blocks[0]
            .spans
            .iter()
            .any(|span| span.text == "mundo" && span.bold));
        assert!(blocks[0]
            .spans
            .iter()
            .any(|span| span.text.contains("Olá") && !span.bold));
    }

    #[test]
    fn ignores_scripts_styles_and_other_executable_content() {
        let blocks = parse_description(
            "<p>Seguro</p><script>alert('x')</script><style>.a{color:red}</style>\
             <iframe src=\"https://evil.test\">frame</iframe><p>Fim</p>",
        );

        assert_eq!(plain(&blocks), vec!["Seguro", "Fim"]);
        let joined = plain(&blocks).join(" ");
        assert!(!joined.contains("alert"));
        assert!(!joined.contains("color:red"));
        assert!(!joined.contains("frame"));
    }

    #[test]
    fn links_show_the_url_and_dangerous_schemes_are_dropped() {
        let blocks = parse_description(
            "<p>Veja <a href=\"https://example.test/x?a=1&amp;b=2\">o site</a>.</p>\
             <p><a href=\"javascript:alert(1)\">não clique</a></p>",
        );

        assert_eq!(
            plain(&blocks),
            vec![
                "Veja o site (https://example.test/x?a=1&b=2).",
                "não clique"
            ]
        );
        assert!(blocks[0]
            .spans
            .iter()
            .any(|span| span.link && span.text.contains("example.test")));
        assert!(!plain(&blocks).join(" ").contains("javascript:"));
    }

    #[test]
    fn lists_get_bullets_and_headings_are_bold() {
        let blocks = parse_description("<h2>Recursos</h2><ul><li>Um</li><li>Dois</li></ul>");

        assert_eq!(plain(&blocks), vec!["Recursos", "• Um", "• Dois"]);
        assert!(blocks[0].spans.iter().all(|span| span.bold));
    }

    #[test]
    fn malformed_html_is_treated_as_text_without_raw_tags() {
        let blocks = parse_description("<div>sem fechamento &lt;tag&gt; 1 < 2");
        assert_eq!(plain(&blocks), vec!["sem fechamento <tag> 1 < 2"]);

        let empty = parse_description("   <p> </p>  ");
        assert!(empty.is_empty());
    }

    #[test]
    fn decodes_common_entities_without_leaking_markup() {
        let blocks = parse_description("<p>&lt;b&gt;não é negrito&lt;/b&gt; &nbsp; fim</p>");
        assert_eq!(plain(&blocks), vec!["<b>não é negrito</b> fim"]);
        assert!(blocks[0].spans.iter().all(|span| !span.bold));
    }
}
