//! Comparação de versões do canal empacotado.
//!
//! Port 1:1 de `compararVersao` em `app/electron/updater-packaged.js`. O
//! oráculo **não** usa semver estrito: cada parte separada por `.` passa por
//! `parseInt(parte, 10) || 0`. Isso significa que:
//!
//! - `"v1.4.10" > "1.4.9"` (numérico, não lexicográfico);
//! - `"1.4" == "1.4.0"` (partes ausentes valem zero);
//! - `"1.4.1-beta" == "1.4.1"` (o `parseInt` para em `-`);
//! - `"1.4.1-rc.2" > "1.4.1"` (vira a parte extra `2`, exatamente como o JS).
//!
//! A função [`parse_semver`] é separada e serve só para validar manifestos
//! (`latest.yml`/`app-update.yml`), onde o electron-updater exige semver.

use semver::Version;

/// Extrai as partes numéricas exatamente como `parseInt(parte, 10) || 0`.
///
/// `parseInt` ignora espaços iniciais, aceita um sinal e para no primeiro
/// caractere que não seja dígito decimal. Sem dígitos o resultado é `NaN`, que
/// o `|| 0` transforma em zero. O `-0` também vira zero.
pub fn version_parts(value: &str) -> Vec<f64> {
    let value = value
        .strip_prefix('v')
        .or_else(|| value.strip_prefix('V'))
        .unwrap_or(value);
    value.split('.').map(parse_int_or_zero).collect()
}

fn parse_int_or_zero(part: &str) -> f64 {
    let text = part.trim_start();
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut sign = 1.0_f64;
    if let Some(&first) = bytes.first() {
        match first {
            b'-' => {
                sign = -1.0;
                index = 1;
            }
            b'+' => index = 1,
            _ => {}
        }
    }
    let start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == start {
        return 0.0;
    }
    let parsed = text[start..index].parse::<f64>().unwrap_or(f64::INFINITY);
    let signed = sign * parsed;
    // `parseInt(...) || 0`: -0 e NaN viram +0.
    if signed == 0.0 {
        0.0
    } else {
        signed
    }
}

/// `compararVersao(a, b)`: devolve `-1`, `0` ou `1`.
///
/// Compara parte a parte até o maior comprimento; partes faltantes valem zero.
pub fn compare_versions(a: &str, b: &str) -> i32 {
    let parts_a = version_parts(a);
    let parts_b = version_parts(b);
    let length = parts_a.len().max(parts_b.len());
    for index in 0..length {
        let left = parts_a.get(index).copied().unwrap_or(0.0);
        let right = parts_b.get(index).copied().unwrap_or(0.0);
        let difference = left - right;
        if difference != 0.0 {
            return if difference > 0.0 { 1 } else { -1 };
        }
    }
    0
}

/// Regra do `checar`: `Boolean(versao) && compararVersao(versao, atual) > 0`.
///
/// Uma candidata vazia nunca é uma atualização; a versão atual vazia é tratada
/// como `0` (mesmo comportamento de `String("")` no JS).
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    !candidate.is_empty() && compare_versions(candidate, current) > 0
}

/// Valida uma versão de manifesto com semver (após remover um `v` inicial).
///
/// O electron-updater só aceita semver nos `latest*.yml`; a comparação em si
/// continua sendo a do oráculo ([`compare_versions`]).
pub fn parse_semver(value: &str) -> Result<Version, semver::Error> {
    let normalized = value
        .trim()
        .strip_prefix('v')
        .or_else(|| value.trim().strip_prefix('V'))
        .unwrap_or_else(|| value.trim());
    Version::parse(normalized)
}

/// Alias em português do oráculo (`compararVersao`).
pub fn comparar_versao(a: &str, b: &str) -> i32 {
    compare_versions(a, b)
}

/// Alias do oráculo para a regra de "há versão nova?".
pub fn versao_mais_nova(candidate: &str, current: &str) -> bool {
    is_newer_version(candidate, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_comparison_is_not_lexicographic() {
        assert_eq!(compare_versions("1.4.10", "1.4.9"), 1);
        assert_eq!(compare_versions("1.4.9", "1.4.10"), -1);
        assert_eq!(compare_versions("v1.4.10", "1.4.9"), 1);
    }

    #[test]
    fn leading_v_is_case_insensitive_and_only_once() {
        assert_eq!(compare_versions("V1.4.1", "v1.4.1"), 0);
        assert_eq!(compare_versions("1.4.1", "v1.4.1"), 0);
        // "vv1.2" -> "v1.2" -> parseInt("v1") = NaN -> 0.
        assert_eq!(compare_versions("vv1.2", "0.2"), 0);
    }

    #[test]
    fn missing_parts_are_zero() {
        assert_eq!(compare_versions("1.4", "1.4.0"), 0);
        assert_eq!(compare_versions("1", "1.0.0"), 0);
        assert_eq!(compare_versions("1.4.0.0.1", "1.4"), 1);
        assert_eq!(compare_versions("", ""), 0);
        assert_eq!(compare_versions("0", ""), 0);
    }

    #[test]
    fn prerelease_follows_parseint_behavior() {
        // O parseInt para no hífen: a parte "1-beta" vale 1.
        assert_eq!(compare_versions("1.4.1-beta", "1.4.1"), 0);
        // "1-rc" vale 1 e o ".2" vira uma quarta parte.
        assert_eq!(compare_versions("1.4.1-rc.2", "1.4.1"), 1);
        assert_eq!(compare_versions("1.4.1-rc.2", "1.4.1.1"), 1);
        assert_eq!(compare_versions("1.4.1+meta", "1.4.1"), 0);
    }

    #[test]
    fn non_numeric_parts_are_zero() {
        assert_eq!(compare_versions("x.1", "0.1"), 0);
        assert_eq!(compare_versions("1..2", "1.0.2"), 0);
        assert_eq!(compare_versions("1.a.3", "1.0.3"), 0);
        assert_eq!(compare_versions(" 1. 2 ", "1.2"), 0);
    }

    #[test]
    fn parse_int_stops_at_first_non_digit() {
        assert_eq!(compare_versions("1e3.0", "1.0"), 0);
        assert_eq!(compare_versions("0x10.0", "0.0"), 0);
        assert_eq!(compare_versions("+2.0", "2.0"), 0);
        assert_eq!(compare_versions("-1.0", "0.0"), -1);
    }

    #[test]
    fn newer_version_requires_non_empty_candidate_and_positive_difference() {
        assert!(is_newer_version("1.4.10", "1.4.9"));
        assert!(!is_newer_version("1.4.9", "1.4.9"));
        assert!(!is_newer_version("1.4.8", "1.4.9"));
        assert!(!is_newer_version("", "1.4.9"));
        assert!(is_newer_version("1.0.0", ""));
        assert!(!is_newer_version("0.0.0", ""));
        assert!(versao_mais_nova("v2", "1.9.9"));
    }

    #[test]
    fn semver_validation_accepts_prerelease_and_v_prefix() {
        assert!(parse_semver("1.4.2").is_ok());
        assert!(parse_semver("v1.4.2").is_ok());
        assert!(parse_semver("1.4.2-beta.1").is_ok());
        assert!(parse_semver("1.4").is_err());
        assert!(parse_semver("").is_err());
    }
}
