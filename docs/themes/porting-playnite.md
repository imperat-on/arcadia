# Portando Temas Playnite para o Arcadia

**Status:** ferramenta assistida disponível  
**Compatibilidade:** reconstrução/conversão, não compatibilidade binária

---

## Visão Geral

O Arcadia não carrega temas Playnite (`.pthm`) diretamente. Em vez disso, oferece uma ferramenta de conversão assistida que analisa um tema Playnite extraído e gera um scaffold Arcadia.

O resultado não é uma conversão perfeita — é um ponto de partida que preserva a identidade visual e lista o que precisa de ajuste manual.

---

## O que pode ser portado

| Nível | Conteúdo | Resultado |
|---|---|---|
| **Visual** | Cores, fontes, bordas, fundos, ícones, imagens, vídeos | Portabilidade praticamente integral |
| **Layout** | Posição de biblioteca, hero, topbar, overview | Alta usando slots e layout declarativo |
| **Comportamento** | Comandos, bindings, add-ons | Exige equivalente no Arcadia ou plugin |

---

## Pré-requisitos

1. **Licença compatível** — verifique se a licença do tema permite redistribuição
2. **Tema extraído** — descompacte o `.pthm` (é um ZIP) em uma pasta
3. **Node.js 18+** — para executar a ferramenta de conversão

---

## Uso da Ferramenta

```bash
node scripts/port-playnite-theme.mjs <pasta-tema-playnite> [pasta-saida]
```

### Exemplo

```bash
# Extrair tema Playnite
unzip my-theme.pthm -d /tmp/playnite-theme

# Converter
node scripts/port-playnite-theme.mjs /tmp/playnite-theme ./my-arcadia-theme

# Resultado:
# ./my-arcadia-theme/theme.json
# ./my-arcadia-theme/theme.css
# ./my-arcadia-theme/assets/
# ./my-arcadia-theme/PORTING_REPORT.md
```

---

## O que a Ferramenta Faz

1. **Lê o manifesto Playnite** (`theme.yaml` ou `theme.json`)
2. **Gera manifesto Arcadia** (`theme.json`) com ID, nome, autor e versão
3. **Analisa views XAML** — encontra todas as views do tema
4. **Extrai cores** — mapeia brushes XAML para tokens CSS
5. **Copia assets** — imagens, vídeos e fontes permitidos
6. **Lista bindings** — identifica `{Binding ...}` que precisam de equivalente
7. **Lista PART_*` — identifica controles que precisam de equivalente
8. **Gera CSS** — arquivo `theme.css` com tokens mapeados
9. **Gera relatório** — `PORTING_REPORT.md` com pendências e próximos passos

---

## O que a Ferramenta NÃO Faz

- ❌ Executa XAML ou WPF
- ❌ Executa scripts C#, PowerShell ou assemblies
- ❌ Importa extensões do Playnite
- ❌ Garante equivalência visual perfeita
- ❌ Converte bindings para código Arcadia
- ❌ Publica ou distribui o tema
- ❌ Ignora licenças

---

## Mapeamento Playnite → Arcadia

| Playnite | Arcadia |
|---|---|
| `theme.yaml` | `theme.json` |
| `ThemeApiVersion` | `themeApiVersion` |
| ResourceDictionary/XAML | Tokens CSS + `theme.css` |
| `GameDetails.xaml` | Superfície `overview` |
| `ThemeFile` | URL `theme://assets/...` |
| `PART_*` | `data-theme-slot` e `data-theme-action` |
| WPF Binding | Props internas do Arcadia |
| MediaElement | Slot de vídeo mudo |
| add-on de tema | Plugin Arcadia separado |
| `.pthm` | `.arcadiatheme` |

---

## Após a Conversão

### 1. Revisar o CSS Gerado

O `theme.css` gerado contém tokens mapeados. Revise e ajuste:

```css
:theme {
  --fs-color-bg: #05070d;
  --fs-color-accent: #72ddff;
  /* ... */
}
```

### 2. Verificar Fontes

Se o tema usa fontes customizadas, inclua-as no pacote:

```
meu-tema/
├── theme.json
├── theme.css
├── assets/
│   ├── MinhaFonte.woff2
│   └── background.webp
└── ...
```

### 3. Mapear Bindings

O relatório lista bindings como `{Binding GameName}`. Estes precisam ser substituídos por componentes do Arcadia:

- `{Binding GameName}` → componente `GameOverview` já fornece
- `{Binding CoverImage}` → slot `overview.cover`
- `{Binding BackgroundImage}` → slot `overview.backdrop`

### 4. Criar Layouts (Opcional)

Se o tema reorganiza a interface, crie layouts:

```json
{
  "schemaVersion": 1,
  "surface": "home",
  "grid": {
    "columns": ["1fr", "360px"],
    "rows": ["auto", "1fr", "auto"],
    "areas": [
      ["topbar", "topbar"],
      ["library", "hero"],
      ["footer", "footer"]
    ]
  },
  "slots": {
    "home.topbar": { "area": "topbar", "required": true },
    "home.library": { "area": "library", "required": true },
    "home.hero": { "area": "hero", "required": true }
  }
}
```

### 5. Testar

- Teste com gamepad (A/B, D-pad, analog)
- Teste em diferentes resoluções (720p, 1080p, 4K)
- Verifique se todos os slots obrigatórios estão visíveis
- Confirme que ações (Jogar, Voltar, Favoritar) funcionam

### 6. Empacotar

```bash
# Compactar como .arcadiatheme
cd meu-tema
zip -r ../meu-tema.arcadiatheme .
```

---

## Recursos Sem Equivalente

Quando um tema depende de recurso ausente no Arcadia:

1. **Mapear para equivalente** — se existe componente similar
2. **Esconder slot** — se o slot é opcional
3. **Implementar no host** — se faz sentido geral
4. **Criar plugin** — se é comportamento especializado
5. **Documentar como não suportado** — se viola segurança ou escopo

---

## Critério de "Portado Corretamente"

Um port correto:

- ✅ Preserva a identidade visual
- ✅ Mantém hierarquia e navegação
- ✅ Usa os mesmos dados disponíveis
- ✅ Não finge suportar painel inexistente
- ✅ Documenta diferenças
- ✅ Respeita licença
- ✅ Funciona sem Playnite instalado
- ✅ Não carrega arquivos do Playnite em runtime
- ✅ Não quebra offline

---

## Exemplo de Relatório

```markdown
# Relatório de Portabilidade Playnite → Arcadia

**Data:** 2026-08-26
**Tema:** My Cool Theme
**ID Arcadia:** ported.my.cool.theme

## Resumo

- Views encontradas: 5
- Cores mapeadas: 12
- Fontes: 2
- Assets copiados: 15
- Bindings Playnite: 23
- PART_*: 8

## Bindings Playnite (pendências)

- `GameName`
- `CoverImage`
- `BackgroundImage`
- `Playtime`
- ...

## Próximos Passos

1. Revisar o `theme.css` gerado
2. Verificar fontes
3. Mapear bindings
4. Criar layouts
5. Testar com gamepad
6. Verificar licença
```

---

## Limitações

- A conversão é assistida, não automática
- Bindings precisam de mapeamento manual
- PART_* podem não ter equivalente direto
- Comportamentos específicos do Playnite não são portáveis
- A fidelidade visual depende do esforço de ajuste
