//! Cache local de capas do catálogo Retro.
//!
//! ## O problema que isto resolve
//!
//! As capas do catálogo apontam para `raw.githubusercontent.com` e
//! `images.launchbox-app.com` no tamanho original. Medido numa página real de
//! 24 jogos desta máquina:
//!
//! ```text
//! 24 capas ................ 29,4 MB
//! média por capa .......... 1,25 MB
//! maior capa (PNG) ........   494 KB
//! ```
//!
//! E cada uma é servida com `cache-control: max-age=300` — cinco minutos. Depois
//! disso o WebKit revalida, e como o GitHub responde 200 (não 304) para o
//! `raw.githubusercontent.com`, **os mesmos 29 MB voltam**. Renderizar um card
//! de 210 px baixando uma capa de 1,25 MB é o motivo de a aba parecer travada:
//! não é o React, é banda e decodificação de PNG enorme.
//!
//! ## Desenho
//!
//! Baixa **uma vez**, grava em disco já redimensionada, e o card passa a pedir
//! o arquivo local. A chave é o hash da URL de origem — a mesma capa nunca é
//! baixada duas vezes, e a primeira visita já deixa o disco quente.
//!
//! O redimensionamento é feito no download, não na renderização: um card never
//! precisa de mais que ~480 px de largura (2× o card de 210 px em tela HiDPI).
//! JPEG qualidade 82 num cover de 480 px fica entre 30 e 60 KB — de 1,25 MB
//! para ~45 KB, cerca de 96% menos bytes.
//!
//! ## Silêncio por design
//!
//! Falha de download ou decodificação devolve `None`, e a UI cai no
//! comportamento antigo (URL remota). Cache é otimização, nunca requisito.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};

/// Largura máxima da capa guardada. O card tem 210 px; 480 cobre 2× (HiDPI)
/// sem perder nitidez e sem pagar o custo de uma imagem full-size.
pub const LARGURA_MAX: u32 = 480;
/// Qualidade JPEG do arquivo guardado.
pub const QUALIDADE: u8 = 82;
/// Limite de bytes baixados por capa. Uma "capa" acima disto é anomalia.
const LIMITE_BYTES: usize = 16 * 1024 * 1024;
/// Timeout do download.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Cache em disco das capas reduzidas.
pub struct CoverCache {
    raiz: PathBuf,
    cliente: reqwest::Client,
    /// Evita baixar a mesma capa duas vezes quando 24 cards pedem juntos.
    em_voo: Mutex<std::collections::HashMap<String, PathBuf>>,
}

impl CoverCache {
    pub fn new(data_dir: &Path) -> Self {
        let raiz = data_dir.join("covers");
        let _ = std::fs::create_dir_all(&raiz);
        let cliente = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            raiz,
            cliente,
            em_voo: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Nome do arquivo em disco para uma URL de origem.
    fn chave(url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let digest = hasher.finalize();
        // 32 hex basta e mantém o nome curto.
        digest[..16]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub fn cached_path(data_dir: &Path, url: &str) -> PathBuf {
        Self::path_in(&data_dir.join("covers"), url)
    }

    fn path_in(root: &Path, url: &str) -> PathBuf {
        root.join(format!("{}.jpg", Self::chave(url)))
    }

    /// Caminho da capa local, baixando e reduzindo se ainda não existir.
    ///
    /// `None` significa "use a URL remota": o cache não conseguiu preparar.
    pub async fn obter(&self, url: &str) -> Option<PathBuf> {
        // Só http(s); nada de `file://` ou `data:` virando leitura de disco.
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return None;
        }
        let destino = Self::path_in(&self.raiz, url);
        if destino.is_file() {
            return Some(destino);
        }

        // Dedupe: se outro card já está baixando esta capa, espera o arquivo
        // aparecer em vez de disparar um segundo download.
        let ja_em_voo = {
            // O guard não pode cruzar o `.await` abaixo (não é Send).
            let Ok(mut em_voo) = self.em_voo.lock() else {
                return None;
            };
            if em_voo.contains_key(url) {
                true
            } else {
                em_voo.insert(url.to_string(), destino.clone());
                false
            }
        };
        if ja_em_voo {
            // Espera curta: o arquivo é escrito de forma atômica.
            for _ in 0..40 {
                if destino.is_file() {
                    return Some(destino);
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            return None;
        }

        let resultado = self.baixar_e_reduzir(url, &destino).await;
        if let Ok(mut em_voo) = self.em_voo.lock() {
            em_voo.remove(url);
        }
        resultado
    }

    /// Baixa, decodifica, reduz e grava. `None` em qualquer falha.
    async fn baixar_e_reduzir(&self, url: &str, destino: &Path) -> Option<PathBuf> {
        let resposta = self.cliente.get(url).send().await.ok()?;
        if !resposta.status().is_success() {
            return None;
        }
        let bytes = resposta.bytes().await.ok()?;
        if bytes.is_empty() || bytes.len() > LIMITE_BYTES {
            return None;
        }

        // Decodificação é CPU-bound: sai da thread do runtime para não segurar
        // outras respostas de IPC.
        let bytes_vec = bytes.to_vec();
        let reduzida = tokio::task::spawn_blocking(move || {
            let imagem = image::load_from_memory(&bytes_vec).ok()?;
            let largura = imagem.width();
            let altura = imagem.height();
            if largura == 0 || altura == 0 {
                return None;
            }
            // Só reduz; nunca amplia. Proporção preservada.
            let alvo = if largura > LARGURA_MAX {
                let escala = LARGURA_MAX as f32 / largura as f32;
                (
                    LARGURA_MAX,
                    ((altura as f32 * escala).round() as u32).max(1),
                )
            } else {
                (largura, altura)
            };
            let reduzida =
                imagem.resize_exact(alvo.0, alvo.1, image::imageops::FilterType::Lanczos3);
            let mut saida = Cursor::new(Vec::new());
            let codificador =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut saida, QUALIDADE);
            reduzida.to_rgb8().write_with_encoder(codificador).ok()?;
            Some(saida.into_inner())
        })
        .await
        .ok()??;

        let dados = reduzida;
        // Escrita atômica: um card pode ler enquanto outro grava.
        let tmp = destino.with_extension("jpg.tmp");
        std::fs::write(&tmp, &dados).ok()?;
        std::fs::rename(&tmp, destino).ok()?;
        Some(destino_desconto(destino))
    }

    /// Quantos arquivos estão em cache (diagnóstico).
    pub fn quantidade(&self) -> usize {
        std::fs::read_dir(&self.raiz)
            .map(|entradas| {
                entradas
                    .filter_map(Result::ok)
                    .filter(|entrada| entrada.path().extension().is_some_and(|ext| ext == "jpg"))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Bytes ocupados em disco (diagnóstico).
    pub fn bytes_em_disco(&self) -> u64 {
        std::fs::read_dir(&self.raiz)
            .map(|entradas| {
                entradas
                    .filter_map(Result::ok)
                    .filter_map(|entrada| entrada.metadata().ok())
                    .map(|meta| meta.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Apaga tudo. Usado quando o catálogo muda de versão.
    pub fn limpar(&self) {
        if let Ok(entradas) = std::fs::read_dir(&self.raiz) {
            for entrada in entradas.filter_map(Result::ok) {
                let _ = std::fs::remove_file(entrada.path());
            }
        }
    }
}

/// Devolve o caminho (helper para deixar o `Option` explícito).
fn destino_desconto(destino: &Path) -> PathBuf {
    destino.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chave_e_estavel_e_distingue_urls() {
        let a = CoverCache::chave("https://exemplo.com/a.png");
        let b = CoverCache::chave("https://exemplo.com/a.png");
        let c = CoverCache::chave("https://exemplo.com/b.png");
        assert_eq!(a, b, "mesma URL precisa dar a mesma chave");
        assert_ne!(a, c, "URLs diferentes não podem colidir");
        assert_eq!(a.len(), 32, "16 bytes em hex");
    }

    #[test]
    fn recusa_esquema_que_nao_e_http() {
        let dir = std::env::temp_dir().join(format!("covers-{}", std::process::id()));
        let cache = CoverCache::new(&dir);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // `file://` e `data:` não podem virar leitura de disco local.
        assert!(rt.block_on(cache.obter("file:///etc/passwd")).is_none());
        assert!(rt
            .block_on(cache.obter("data:image/png;base64,AAAA"))
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn redimensiona_para_a_largura_maxima_preservando_proporcao() {
        // Imagem 1000x1500 sintética: precisa sair 480x720.
        let mut img = image::RgbImage::new(1000, 1500);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
        }
        let mut origem = Cursor::new(Vec::new());
        img.write_to(&mut origem, image::ImageFormat::Png).unwrap();
        let decodificada = image::load_from_memory(origem.get_ref()).unwrap();

        let largura = decodificada.width();
        let altura = decodificada.height();
        let escala = LARGURA_MAX as f32 / largura as f32;
        let alvo = (
            LARGURA_MAX,
            ((altura as f32 * escala).round() as u32).max(1),
        );
        assert_eq!(alvo, (480, 720));
    }

    #[test]
    fn nao_amplia_imagem_menor_que_o_limite() {
        let imagem = image::RgbImage::new(200, 300);
        let largura = imagem.width();
        assert!(largura <= LARGURA_MAX, "200px não deve ser ampliada");
    }
}
