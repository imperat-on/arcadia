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
    /// Imagem inline da descrição (`<img src>` ou `<video poster>`), quando o
    /// bloco é uma figura. Os cards de seção da Steam ("Explore a Land of Vast
    /// Wonders") vivem aqui: o HTML da loja usa mídia solta no meio do texto.
    pub image: Option<String>,
    /// Mídia de vídeo do mesmo card (`<source src="….webm">`).
    ///
    /// Existe porque o `poster` nem sempre tem arte: o Black Myth: Wukong
    /// manda um AVIF preto de 336 bytes. Com o vídeo em mãos, a UI extrai o
    /// primeiro frame visível e mostra a imagem de verdade.
    pub video: String,
    /// Bloco veio de `<h1>`-`<h6>`: a UI desenha com corpo e peso de título.
    pub heading: bool,
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
        self.image.is_some() || self.spans.iter().any(|span| !span.text.trim().is_empty())
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
    /// Acabamos de abrir um `<video>`; o próximo `<source>` é a mídia dele.
    pending_video: bool,
    /// Todas as mídias de vídeo vistas, na ordem. Serve para a UI saber
    /// quais cards precisam de extração de frame.
    video_sources: Vec<String>,
}

struct LinkState {
    href: String,
    span_start: usize,
}

impl Parser {
    fn run(&mut self, input: &str) {
        // O `about` da Steam mistura HTML com BBCode: `<img src>` e
        // `[img]url[/img]` convivem no mesmo texto. O BBCode é convertido
        // para tags equivalentes antes do parser, para as figuras de seção
        // ("Digital Deluxe Edition", "Explore a Land of Vast Wonders")
        // virarem bloco de imagem nos dois formatos.
        let normalized = normalize_bbcode(input);
        let input = normalized.as_str();

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
            "img" | "video" => {
                if tag.close {
                    // `</video>` fecha o card: o bloco de imagem que estava
                    // aberto vai para a lista com o vídeo já anexado.
                    self.pending_video = false;
                    self.finalize_block();
                    return;
                }
                // O HTML da loja põe os cards de seção como mídia solta entre
                // parágrafos. Cada uma fecha o bloco atual e vira um bloco
                // próprio, para a UI desenhá-la na largura do painel.
                //
                // `video` é obrigatório: o `about` do Cyberpunk 2077 usa
                // `<video class="bb_img" poster="….avif">` para TODOS os seis
                // cards de seção — sem esta tag, a descrição fica sem nenhuma
                // figura.
                //
                // O `poster` é o frame estático e é o que desenhamos. Mas ele
                // nem sempre tem arte: o Black Myth: Wukong entrega um PNG
                // preto de 336 bytes. Por isso o bloco guarda também o vídeo —
                // a UI extrai um frame real quando o poster é preto.
                let poster = if tag.name == "video" {
                    src_from_attrs_any(&tag.attrs, &["poster"])
                } else {
                    src_from_attrs(&tag.attrs)
                };
                if let Some(src) = poster {
                    self.finalize_block();
                    self.current.image = Some(src);
                    if tag.name == "video" {
                        // O `<source>` vem DEPOIS no stream, então o bloco
                        // continua aberto até o `</video>`. Guardamos a
                        // posição para o vídeo se ligar a ele.
                        self.pending_video = true;
                    } else {
                        self.finalize_block();
                    }
                } else if tag.name == "video" {
                    // Vídeo sem poster: o `<source>` filho é a única fonte.
                    self.pending_video = true;
                }
                return;
            }
            "source" => {
                // `<source>` dentro de `<video>`: é a mídia em si (webm/mp4).
                // Sem poster, o vídeo é a única fonte possível e o primeiro
                // frame é extraído dele; com poster, o vídeo fica guardado
                // como reserva para o caso de o poster ser preto.
                if self.pending_video {
                    if let Some(src) = src_from_attrs(&tag.attrs) {
                        let lower = src.to_ascii_lowercase();
                        if lower.contains(".webm") || lower.contains(".mp4") {
                            self.video_sources.push(src.clone());
                            // O bloco de figura ainda está aberto (o `</video>`
                            // ainda não chegou). O primeiro `<source>` vence:
                            // a Steam lista webm antes de mp4, e VP9 decodifica
                            // sem os codecs restritos do H.264.
                            if self.current.video.is_empty() {
                                self.current.video = src;
                            }
                        }
                    }
                }
                return;
            }
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
                // Marca o próximo bloco como título; `finalize_block` limpa
                // a flag ao fechar.
                if !tag.close {
                    self.current.heading = true;
                }
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

    sanitize_url(value)
}

/// `src` de um `<img>`. Mesma higiene de esquema aplicada a `href`: só
/// `https`, `http` e caminhos relativos sobrevivem.
fn src_from_attrs(attrs: &str) -> Option<String> {
    src_from_attrs_any(attrs, &["src"])
}

/// Lê o valor de qualquer um dos atributos em `names`, na ordem dada.
///
/// `src_from_attrs` só olha `src`, mas `<video poster="…">` guarda a imagem
/// em `poster` — sem isso todos os cards da descrição do Cyberpunk 2077
/// seriam descartados.
fn src_from_attrs_any(attrs: &str, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(value) = attribute_value(attrs, name) {
            if let Some(url) = sanitize_url(&value) {
                return Some(url);
            }
        }
    }
    None
}

/// Valor cru de um atributo HTML, sem decodificar entidades.
///
/// Casa o nome exato para não confundir `src` com `data-src` ou `srcset`
/// (o `find("src")` antigo pegava `data-src` primeiro quando ele vinha antes).
fn attribute_value(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let bytes_lower = lower.as_bytes();
    let name_bytes = name.as_bytes();
    let mut index = 0;

    while index + name_bytes.len() <= bytes_lower.len() {
        let Some(found) = lower[index..].find(name) else {
            return None;
        };
        let start = index + found;
        let before_ok = start == 0
            || !bytes_lower[start - 1].is_ascii_alphanumeric()
                && bytes_lower[start - 1] != b'-'
                && bytes_lower[start - 1] != b'_';
        let end = start + name_bytes.len();
        let after_ok = end >= bytes_lower.len()
            || !bytes_lower[end].is_ascii_alphanumeric() && bytes_lower[end] != b'-';

        if before_ok && after_ok {
            let mut rest = &attrs[end..];
            rest = rest.trim_start();
            if let Some(stripped) = rest.strip_prefix('=') {
                let rest = stripped.trim_start();
                let value = if let Some(stripped) = rest.strip_prefix('"') {
                    stripped[..stripped.find('"')?].to_string()
                } else if let Some(stripped) = rest.strip_prefix('\'') {
                    stripped[..stripped.find('\'')?].to_string()
                } else {
                    rest.split_whitespace().next()?.to_string()
                };
                return Some(value);
            }
        }

        index = end;
    }

    None
}

fn sanitize_url(value: &str) -> Option<String> {
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
        return Some(format!("https://{rest}"));
    }
    Some(value.to_string())
}

/// Converte BBCode de imagem em `<img src="...">`.
///
/// O `about` da Steam usa os dois dialetos ao mesmo tempo. Formatos aceitos:
/// - `[img]https://…[/img]`
/// - `[img src="https://…"][/img]`
/// - `[img=width]https://…[/img]`
///
/// Qualquer outra tag BBCode (`[b]`, `[url]`, `[list]`, `[h1]`) vira texto —
/// o parser já lida com o HTML ao redor, e descartar o marcador é melhor do
/// que vazá-lo para a tela.
fn normalize_bbcode(input: &str) -> String {
    if !input.contains("[img") {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len() + 32);
    let mut rest = input;

    while let Some(start) = rest.find("[img") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];

        let Some(close_bracket) = after.find(']') else {
            // `[img` sem fechar: trata como texto literal.
            out.push_str(after);
            return out;
        };
        let header = &after[..=close_bracket];
        let body_start = close_bracket + 1;

        let Some(close_tag) = after[body_start..].find("[/img]") else {
            out.push_str(after);
            return out;
        };
        let body = &after[body_start..body_start + close_tag];
        let consumed = body_start + close_tag + "[/img]".len();

        // `[img]url[/img]` ou `[img=200]url[/img]` -> o corpo é a URL.
        // `[img src="url"][/img]` -> a URL está no cabeçalho.
        let url = if let Some(src) = src_from_attrs(header) {
            src
        } else {
            body.trim().to_string()
        };

        if !url.is_empty() {
            out.push_str("<img src=\"");
            out.push_str(&url.replace('"', ""));
            out.push_str("\">");
        }

        rest = &after[consumed..];
    }

    out.push_str(rest);
    out
}

/// URLs de imagem embutidas na descrição, na ordem em que aparecem.
///
/// O app usa `parse_description` diretamente (precisa do par poster/vídeo de
/// cada card); esta função existe para testes e para consumidores que só
/// querem a lista de imagens. A UI pede esses handles ao
/// [`crate::covers::CoverCache`] junto das capas, para os cards de seção da
/// Steam ("Explore a Land of Vast Wonders") saírem com a arte original.
#[cfg_attr(not(test), allow(dead_code))]
pub fn description_images(html: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for block in parse_description(html) {
        if let Some(url) = block.image {
            if !urls.contains(&url) {
                urls.push(url);
            }
        }
    }
    urls
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

    #[test]
    fn keeps_inline_images_as_their_own_blocks() {
        // Formato real do `about` da Steam: parágrafo, card de seção, parágrafo.
        let blocks = parse_description(
            "<p>Intro do jogo.</p>\
             <img src=\"https://cdn.test/wukong/wonders.jpg\" \
                  srcset=\"https://cdn.test/wukong/wonders_2x.jpg 2x\">\
             <h2>Explore a Land of Vast Wonders</h2><p>Texto da seção.</p>",
        );

        let images: Vec<Option<String>> = blocks.iter().map(|block| block.image.clone()).collect();
        assert_eq!(
            images,
            vec![
                None,
                Some("https://cdn.test/wukong/wonders.jpg".to_string()),
                None,
                None
            ]
        );
        // A imagem não vaza para o texto dos parágrafos: o bloco dela é vazio
        // e os parágrafos vizinhos mantêm o próprio texto.
        assert_eq!(
            plain(&blocks),
            vec![
                "Intro do jogo.",
                "",
                "Explore a Land of Vast Wonders",
                "Texto da seção."
            ]
        );
        assert_eq!(
            description_images(
                "<p>a</p><img src=\"https://cdn.test/1.jpg\"><img src='https://cdn.test/2.jpg'>"
            ),
            vec![
                "https://cdn.test/1.jpg".to_string(),
                "https://cdn.test/2.jpg".to_string()
            ]
        );
    }

    #[test]
    fn image_sources_reject_dangerous_schemes_and_protocol_relative() {
        assert!(description_images("<img src=\"javascript:alert(1)\">").is_empty());
        assert!(description_images("<img src=\"data:image/png;base64,AAAA\">").is_empty());
        assert!(description_images("<img>").is_empty());
        assert!(description_images("<img alt=\"sem src\">").is_empty());
        // `//` vira https, como no `href`.
        assert_eq!(
            description_images("<img src=\"//cdn.test/x.png\">"),
            vec!["https://cdn.test/x.png".to_string()]
        );
    }

    #[test]
    fn extracts_video_posters_from_the_store_about() {
        // O `about` do Cyberpunk 2077 usa `<video class="bb_img" poster="…">`
        // para os seis cards de seção, não `<img>`. Antes de tratar `poster`,
        // a descrição do jogo saía sem nenhuma figura.
        let about = concat!(
            "<span class=\"bb_img_ctn\">",
            "<video class=\"bb_img\" autoplay muted loop playsinline ",
            "poster=\"https://shared.test/extras/aaa.poster.avif\" ",
            "width=1170 height=450 >",
            "<source src=\"https://shared.test/extras/aaa.webm\">",
            "<source src=\"https://shared.test/extras/aaa.mp4\">",
            "</video></span>",
            "<span class=\"bb_img_ctn\">",
            "<video class=\"bb_img\" poster=\"https://shared.test/extras/bbb.poster.avif\">",
            "<source src=\"https://shared.test/extras/bbb.webm\">",
            "</video></span>"
        );

        assert_eq!(
            description_images(about),
            vec![
                "https://shared.test/extras/aaa.poster.avif".to_string(),
                "https://shared.test/extras/bbb.poster.avif".to_string(),
            ],
            "os posters dos vídeos são os cards de seção da descrição"
        );
    }

    #[test]
    fn names_bbcode_images_and_keeps_html_ones() {
        // A Steam mistura os dois dialetos no mesmo `about`. Os dois têm que
        // virar figura; o BBCode não pode vazar como texto literal.
        let about = concat!(
            "<h1>Deluxe</h1>",
            "[img]https://shared.test/a.webp[/img]",
            "texto",
            "<img src=\"https://shared.test/b.webp\">"
        );

        assert_eq!(
            description_images(about),
            vec![
                "https://shared.test/a.webp".to_string(),
                "https://shared.test/b.webp".to_string(),
            ]
        );
    }

    #[test]
    fn attaches_the_video_source_to_the_card_block() {
        // O card do Wukong é `<video poster="….avif"><source src="….webm">`.
        // O poster é preto; sem o `video` no bloco a UI não tem de onde tirar
        // a imagem real.
        let about = concat!(
            "<span class=\"bb_img_ctn\">",
            "<video class=\"bb_img\" poster=\"https://shared.test/a.poster.avif\">",
            "<source src=\"https://shared.test/a.webm\" type=\"video/webm\">",
            "<source src=\"https://shared.test/a.mp4\" type=\"video/mp4\">",
            "</video></span>"
        );

        let blocks = parse_description(about);
        let figure = blocks
            .iter()
            .find(|block| block.image.is_some())
            .expect("bloco de figura presente");

        assert_eq!(
            figure.image.as_deref(),
            Some("https://shared.test/a.poster.avif")
        );
        assert_eq!(
            figure.video, "https://shared.test/a.webm",
            "o vídeo fica guardado como reserva do poster preto"
        );
    }

    #[test]
    fn image_without_video_has_empty_video_field() {
        let about = "<img src=\"https://shared.test/banner.webp\">";
        let blocks = parse_description(about);
        let figure = blocks
            .iter()
            .find(|block| block.image.is_some())
            .expect("bloco de figura presente");

        assert_eq!(
            figure.image.as_deref(),
            Some("https://shared.test/banner.webp")
        );
        assert!(figure.video.is_empty());
    }

    #[test]
    fn marks_heading_blocks_from_h_tags() {
        let blocks = parse_description("<h1>About the Game</h1><p>Corpo</p>");
        let heading = blocks
            .iter()
            .find(|block| block.spans.iter().any(|span| span.text.contains("About")))
            .expect("bloco de título presente");
        assert!(heading.heading, "h1 marca o bloco como título");

        let body = blocks
            .iter()
            .find(|block| block.spans.iter().any(|span| span.text.contains("Corpo")))
            .expect("bloco de corpo presente");
        assert!(!body.heading, "parágrafo comum não é título");
    }

    #[test]
    fn extracts_every_card_from_the_real_store_about() {
        // HTML real do `about` do Black Myth: Wukong (appid 2358720),
        // capturado do app: 2 `<img src>` + 4 `<video poster>`. Antes de
        // tratar `<video poster>`, só os 2 `<img>` viravam figura.
        let about = "<h1>Digital Deluxe Edition</h1><p><span class=\"bb_img_ctn\"><img class=\"bb_img\" src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/b7cb474dd8f070fe6643a85cc8c95b38.avif?t=1760601605\" width=1170 height=438 /></span><br><br>The Black Myth: Wukong Digital Deluxe Edition includes:<br>- Black Myth: Wukong Full Game<br>- Weapon: Bronzecloud Staff<br>- Equipment: Folk Opera Mask, Folk Opera Almsgiving Armor, Folk Opera Leather Bracers, and Folk Opera Buskins<br>- Curio: Wind Chimes<br>- Selected Digital Soundtrack<br><br>※ The included weapon, equipment, and curio can be claimed from the Trailblazer's Gift menu at the Keeper's Shrine (Respawn Point) during the early stages of the game.<br>※ By activating &quot;Music Library&quot; on the game's Title Screen, or downloading &quot;Black Myth: Wukong Soundtrack Selection&quot; through the platform, you can enjoy a curated list of tracks from the game. We highly recommend that you first experience these pieces as you journey through the game as the Destined One, then return here to relive them to your heart's content.</p><br><h1>About the Game</h1>Black Myth: Wukong is an action RPG rooted in Chinese mythology. The story is based on Journey to the West, one of the Four Great Classical Novels of Chinese literature. You shall set out as the Destined One to venture into the challenges and marvels ahead, to uncover the obscured truth beneath the veil of a glorious legend from the past.<br><br><span class=\"bb_img_ctn\"><img class=\"bb_img\" src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/53c062bc38fc59440d6e9ae53e6e51f2.webp?t=1760601605\" width=1170 height=27 /></span><br><br><span class=\"bb_img_ctn\"><video class=\"bb_img\" autoplay muted loop playsinline poster=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/6d5d441bd83627bcbc6d2b8f8d15f0ff.poster.avif?t=1760601605\" width=1170 height=438 ><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/6d5d441bd83627bcbc6d2b8f8d15f0ff.webm?t=1760601605\" type=\"video/webm; codecs=vp9\"><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/6d5d441bd83627bcbc6d2b8f8d15f0ff.mp4?t=1760601605\" type=\"video/mp4\"></video></span><h2 class=\"bb_tag\" >• Explore a Land of Vast Wonders</h2><br><i>&quot;A world unseen, where wonders gleam,<br>And with each stride, a new scene streams.&quot;</i><br><br>Enter a fascinating realm filled with the wonders and discoveries of ancient Chinese mythology!<br>As the Destined One, you shall traverse an array of breathtaking and distinctive landscapes from the classic tale, composing an epic of adventure that is seen anew.<br><br><br><span class=\"bb_img_ctn\"><video class=\"bb_img\" autoplay muted loop playsinline poster=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/50a130565eba20a23c614617374844e6.poster.avif?t=1760601605\" width=1170 height=438 ><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/50a130565eba20a23c614617374844e6.webm?t=1760601605\" type=\"video/webm; codecs=vp9\"><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/50a130565eba20a23c614617374844e6.mp4?t=1760601605\" type=\"video/mp4\"></video></span><h2 class=\"bb_tag\" >• Confront Mighty Foes, Old and New</h2><br><i>&quot;Heroic Monkey, might and fame, <br>Adversaries rise, to test his name.&quot;</i><br><br>One of the major highlights of Journey to the West is its diverse cast of adversaries, each with their unique strengths.<br>As the Destined One, you will encounter powerful foes and worthy rivals throughout your journey. Fearlessly engage them in epic battles where surrender is not an option.<br><br><br><span class=\"bb_img_ctn\"><video class=\"bb_img\" autoplay muted loop playsinline poster=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/cac7e67395cc9fbe1463054a2004496a.poster.avif?t=1760601605\" width=1170 height=438 ><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/cac7e67395cc9fbe1463054a2004496a.webm?t=1760601605\" type=\"video/webm; codecs=vp9\"><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/cac7e67395cc9fbe1463054a2004496a.mp4?t=1760601605\" type=\"video/mp4\"></video></span><h2 class=\"bb_tag\" >• Temper Your Mastery of Varied Spells</h2><br><i>&quot;Spells unbound, talents in play,<br>Unleashed abilities seize the day.&quot;</i><br><br>Spells, transformations, and magic vessels of all manifestations, in which some counteract while others amplify, have long been iconic combat elements of Chinese mythology.<br>As the Destined One, aside from mastering various staff techniques, you can also freely combine different spells, abilities, weapons, and equipment to find a winning strategy that best suits your combat style.<br><br><br><span class=\"bb_img_ctn\"><video class=\"bb_img\" autoplay muted loop playsinline poster=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/717266ac34d8670ca14c77550aad5787.poster.avif?t=1760601605\" width=1170 height=438 ><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/717266ac34d8670ca14c77550aad5787.webm?t=1760601605\" type=\"video/webm; codecs=vp9\"><source src=\"https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/2358720/extras/717266ac34d8670ca14c77550aad5787.mp4?t=1760601605\" type=\"video/mp4\"></video></span><h2 class=\"bb_tag\" >• Discover Heartfelt Tales Behind Every Facade</h2><br><i>&quot;Within all souls, both wild and tame,<br>There thrives a tale of life's fierce flame.&quot;</i><br><br>Beneath the ferocity and craftiness of your foes lies an engaging tapestry of their origins, personalities, and motivations waiting to be revealed.<br>As the Destined One, you will uncover the stories behind a variety of characters, delving beyond your battles with them to taste the love, hate, greed, and fury they once held and still carry within.";
        let found = description_images(about);
        assert_eq!(found.len(), 6, "2 <img> + 4 posters de vídeo: {found:#?}");
        assert!(
            found
                .iter()
                .all(|u| u.contains("shared.akamai.steamstatic.com")),
            "todas as URLs vêm do CDN da Steam"
        );
    }
}
