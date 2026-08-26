# API Visual de Temas Fullscreen — v1

**Versão da API:** 1  
**Versão do manifesto:** 1  
**Escopo:** modo Big Picture (console) do Arcadia  
**Status:** implementação funcional — 98 testes automatizados passando

---

## 1. Formato do pacote

Extensão: `.arcadiatheme`  
Formato: ZIP com extensão própria  
Manifesto obrigatório na raiz: `theme.json`

```
arcadia-aurora.arcadiatheme
├── theme.json
├── theme.css
├── layouts/
│   ├── home.json
│   └── overview.json
├── assets/
│   ├── background.webp
│   ├── frame.png
│   ├── focus.png
│   ├── display.woff2
│   └── ambient.webm
├── previews/
│   ├── home.webp
│   └── overview.webp
├── LICENSE
└── README.md
```

Layouts são opcionais. Sem eles, o tema usa o layout padrão e personaliza somente por CSS.

---

## 2. Manifesto (`theme.json`)

```json
{
  "manifestVersion": 1,
  "themeApiVersion": 1,
  "id": "arcadia.aurora",
  "name": "Arcadia Aurora",
  "author": "Arcadia",
  "version": "1.0.0",
  "description": "Tema cinematográfico azul e violeta.",
  "mode": "fullscreen",
  "entry": "theme.css",
  "layouts": {
    "home": "layouts/home.json",
    "overview": "layouts/overview.json"
  },
  "previews": [
    "previews/home.webp",
    "previews/overview.webp"
  ],
  "features": [
    "tokens",
    "assets",
    "layout:home",
    "layout:overview"
  ],
  "options": {
    "railPosition": { "type": "enum", "values": ["top", "bottom"], "default": "top" },
    "showClock": { "type": "boolean", "default": true },
    "blurIntensity": { "type": "intensity", "min": 0, "max": 1, "default": 0.5 },
    "accentColor": { "type": "color", "default": "#72ddff" },
    "padding": { "type": "number", "min": 0, "max": 64, "default": 16 }
  },
  "supports": {
    "minWidth": 1280,
    "minHeight": 720,
    "aspectRatios": ["16:9", "16:10", "21:9"]
  },
  "license": "MIT",
  "homepage": "https://example.invalid/arcadia-aurora"
}
```

### Regras do manifesto

| Campo | Regra |
|---|---|
| `manifestVersion` | Deve ser exatamente `1` |
| `themeApiVersion` | Não pode ser maior que a API do host |
| `id` | Minúsculo, até 64 caracteres, `[a-z0-9._-]` |
| `version` | SemVer obrigatório |
| `mode` | Deve ser `fullscreen` |
| `entry` | Arquivo `.css` dentro do pacote, sem traversal |
| `layouts` | Chaves devem ser `home` ou `overview` |
| `previews` | Máximo 16 imagens |
| `features` | Máximo 64, desconhecidas são ignoradas |
| `options` | Máximo 64 opções |

Campos desconhecidos são preservados para diagnóstico, nunca interpretados.

### Compatibilidade de API

| Situação | Comportamento |
|---|---|
| API igual | Carrega normalmente |
| API menor suportada | Carrega com compatibilidade |
| API maior | Marca como incompatível; não ativa |
| Manifesto inválido | Recusa instalação |
| Tema ativo removido/corrompido | Volta ao tema embutido |

---

## 3. Tokens CSS obrigatórios

### Cor

| Token | Descrição |
|---|---|
| `--fs-color-bg` | Fundo principal |
| `--fs-color-surface` | Superfície de cards |
| `--fs-color-surface-strong` | Superfície destacada |
| `--fs-color-text` | Texto principal |
| `--fs-color-muted` | Texto secundário |
| `--fs-color-accent` | Cor de destaque primária |
| `--fs-color-accent-2` | Cor de destaque secundária |
| `--fs-color-danger` | Cor de erro/perigo |

### Foco

| Token | Descrição |
|---|---|
| `--fs-focus-color` | Cor do anel de foco |
| `--fs-focus-width` | Largura do anel |
| `--fs-focus-offset` | Distância do elemento |
| `--fs-focus-glow` | Brilho do foco |

### Tipografia

| Token | Descrição |
|---|---|
| `--fs-font-body` | Fonte do corpo |
| `--fs-font-display` | Fonte de destaque |
| `--fs-font-action` | Fonte de botões |
| `--fs-font-scale` | Escala global da fonte |

### Forma

| Token | Descrição |
|---|---|
| `--fs-radius-sm` | Raio pequeno |
| `--fs-radius-md` | Raio médio |
| `--fs-radius-lg` | Raio grande |
| `--fs-border-color` | Cor de borda padrão |

### Superfície

| Token | Descrição |
|---|---|
| `--fs-glass-bg` | Fundo glassmorphism |
| `--fs-glass-blur` | Blur do glass |
| `--fs-shadow-panel` | Sombra de painéis |
| `--fs-shadow-focus` | Sombra de foco |

### Espaço

| Token | Descrição |
|---|---|
| `--fs-space-1` a `--fs-space-8` | Escala de espaçamento |
| `--fs-safe-x` | Área segura horizontal |
| `--fs-safe-y` | Área segura vertical |

### Movimento

| Token | Descrição |
|---|---|
| `--fs-motion-fast` | Duração rápida |
| `--fs-motion-normal` | Duração normal |
| `--fs-motion-slow` | Duração lenta |
| `--fs-ease-enter` | Curva de entrada |
| `--fs-ease-exit` | Curva de saída |

### Biblioteca

| Token | Descrição |
|---|---|
| `--fs-cover-width` | Largura das capas |
| `--fs-cover-ratio` | Proporção das capas |
| `--fs-rail-gap` | Espaço entre itens do trilho |
| `--fs-selected-scale` | Escala do item selecionado |

### Precedência

1. Regras de segurança do host
2. Acessibilidade / reduced motion
3. Preferências do usuário
4. CSS do tema ativo
5. Tokens default do host

---

## 4. Slots estáveis

### Shell e home

- `shell.root`
- `shell.backdrop`
- `shell.overlay`
- `home.topbar`
- `home.navigation`
- `home.library`
- `home.rail`
- `home.game-card`
- `home.hero`
- `home.hero.logo`
- `home.hero.description`
- `home.hero.actions`
- `home.info`
- `home.footer`

### Overview

- `overview.root`
- `overview.backdrop`
- `overview.topbar`
- `overview.cover`
- `overview.identity`
- `overview.tags`
- `overview.description`
- `overview.actions`
- `overview.progress`
- `overview.media`
- `overview.metadata`
- `overview.activities`

### Superfícies secundárias

- `news.root`, `news.featured`, `news.rail`
- `store.root`, `store.header`, `store.content`
- `settings.root`, `settings.navigation`, `settings.content`
- `profile.root`, `profile.card`
- `downloads.root`, `downloads.item`
- `dialog.root`, `dialog.actions`
- `toast.root`

---

## 5. Estados estáveis

O host fornece estados sem colocar dados sensíveis em atributos:

- `data-theme-state="selected"`
- `data-theme-state="running"`
- `data-theme-state="opening"`
- `data-theme-state="installed"`
- `data-theme-state="missing"`
- `data-theme-state="favorite"`
- `data-theme-state="active"`
- `data-theme-state="disabled"`

---

## 6. Ações estáveis

Botões pertencentes ao host recebem:

- `data-theme-action="launch"`
- `data-theme-action="stop"`
- `data-theme-action="install"`
- `data-theme-action="details"`
- `data-theme-action="back"`
- `data-theme-action="favorite"`
- `data-theme-action="settings"`
- `data-theme-action="downloads"`

O atributo serve para estilo e composição. O tema não registra handlers.

---

## 7. Assets

Autores usam:

```css
url("theme://assets/background.webp")
```

O serviço reescreve para `arcadia-theme://<id>/assets/background.webp` e serve apenas arquivos allowlisted.

**Não aceitos em tema externo:** `http:`, `https:`, `file:`, `data:`, `blob:`, `@import`.

### MIME permitidos

| Extensão | MIME |
|---|---|
| `.png` | `image/png` |
| `.jpg`, `.jpeg` | `image/jpeg` |
| `.webp` | `image/webp` |
| `.gif` | `image/gif` |
| `.woff2` | `font/woff2` |
| `.mp4` | `video/mp4` |
| `.webm` | `video/webm` |

---

## 8. Layout declarativo

```json
{
  "schemaVersion": 1,
  "surface": "home",
  "grid": {
    "columns": ["minmax(0, 1fr)", "360px"],
    "rows": ["auto", "minmax(0, 1fr)", "auto"],
    "areas": [
      "topbar topbar",
      "library hero",
      "footer footer"
    ]
  },
  "slots": {
    "home.topbar": { "area": "topbar", "required": true },
    "home.library": { "area": "library", "required": true },
    "home.hero": { "area": "hero", "required": true },
    "home.footer": { "area": "footer", "required": false }
  }
}
```

### Validação

- Profundidade máxima: 8
- Máximo de áreas: 32
- Máximo de slots: 64
- Somente slots conhecidos
- Tracks CSS em gramática restrita
- Slots obrigatórios não podem ser escondidos
- Nenhum texto HTML, evento ou binding arbitrário
- Fallback para layout padrão se a tela não couber

---

## 9. Opções do tema

Tipos suportados: `boolean`, `enum`, `number`, `color`, `intensity`.

As opções viram atributos `data-theme-option-*` ou variáveis `--theme-option-*`. Nunca chamam código do tema.

---

## 10. Escopo CSS

O seletor documental do tema é `:theme`. O serviço transforma no seletor escopado ao ID ativo. Seletores fora de `:theme` são rejeitados.

```css
:theme {
  --fs-color-accent: #72ddff;
}

:theme [data-theme-slot="home.hero"] {
  backdrop-filter: blur(18px);
}
```

### Rejeitado

- `@import`
- URLs externas
- URLs `file:`, `data:` ou `blob:`
- Seletor não escopado a `:theme`
- Propriedades proprietárias Electron
- Folhas acima de 2 MiB
- Sintaxe inválida

---

## 11. Arquivos proibidos

Tema externo v1 não pode conter:

- `.js`, `.mjs`, `.cjs`, `.ts`, `.tsx`, `.jsx`
- `.html`, `.htm`
- `.wasm`
- Executáveis (`.exe`, `.dll`, `.so`, `.dylib`)
- Shell scripts (`.sh`, `.bat`, `.cmd`, `.ps1`)
- `.svg` sem sanitização
- Links simbólicos ou hardlinks

---

## 12. Limites do pacote

| Recurso | Limite |
|---|---|
| Pacote comprimido | 128 MiB |
| Conteúdo descompactado | 256 MiB |
| Máximo de arquivos | 2.048 |
| Manifesto | 64 KiB |
| CSS total | 2 MiB |
| Layout individual | 256 KiB |
| Imagem individual | 32 MiB |
| Vídeo individual | 128 MiB |
| Fonte individual | 8 MiB |
| Profundidade de diretório | 12 |

---

## 13. Temas embutidos

| ID | Descrição |
|---|---|
| `arcadia.default` | Aparência atual do Big Picture (fallback permanente) |
| `arcadia.aurora` | Tema cinematográfico azul/violeta (tema demonstrativo) |

Built-ins aparecem na listagem, não podem ser removidos e servem como fallback.

---

## 14. Raiz do documento

O Big Picture expõe:

```html
<div
  data-arcadia-mode="console"
  data-fullscreen-theme="arcadia.aurora"
  data-theme-api="1"
  data-theme-ready="true"
>
```

---

## 15. IPC público

| Método | Resultado |
|---|---|
| `fullscreenThemesList()` | Descritores públicos e tema ativo |
| `fullscreenThemeGet(id)` | Descritor público de um tema |
| `fullscreenThemesGetPayload(id)` | Payload seguro com CSS normalizado, layouts e opções |
| `fullscreenThemesActivate(id)` | Ativa tema válido como pendente |
| `fullscreenThemesConfirmReady(id)` | Confirma health check (deve usar exatamente o pendingId) |
| `fullscreenThemesRollbackPending()` | Desfaz ativação pendente |
| `fullscreenThemesImport()` | Abre seletor, valida ZIP, instala e retorna `{ ok, id, version, error }` |
| `fullscreenThemesRemove(id)` | Remove tema externo não ativo |
| `fullscreenThemesRecover()` | Recupera para o último tema saudável |
| `fullscreenThemesGetActiveId()` | Retorna o ID do tema ativo |
| `onFullscreenThemesChanged(cb)` | Evento com `{ reason, activeId, pendingId, changedId }` |

### Payload seguro

O payload retornado por `getPayload` contém:

- `id` — ID do tema
- `name` — Nome do tema
- `themeApiVersion` — Versão da API
- `css` — CSS já normalizado e escopado (seletores `:theme` → `[data-fullscreen-theme="id"]`)
- `cssErrors` — Erros de normalização (não fatais)
- `layouts` — Layouts declarativos validados por superfície
- `options` — Opções resolvidas (defaults + preferências do usuário)
- `previews` — URLs de preview normalizadas (`arcadia-theme://`)
- `compat` — Status de compatibilidade
- `source` — `"builtin"` ou `"local"`

Nunca devolve paths locais, diretórios do tema, conteúdo não validado ou tokens.

---

## 16. Segurança

- Nenhum JavaScript executável em temas
- Nenhum path local exposto ao renderer
- Nenhuma URL remota em CSS
- Traversal, symlink, zip bomb e executáveis rejeitados
- CSP permite `arcadia-theme:` apenas em `img-src`, `media-src` e `font-src`
- Tema com falha não bloqueia boot (fallback para built-in)
- Modo seguro: `--safe-theme` ou `ARCADIA_SAFE_THEME=1`

---

## 17. Acessibilidade

Requisitos obrigatórios:

- Foco visível imposto pelo host
- Ordem DOM coerente mesmo quando CSS muda posição
- A/B, Enter/Escape continuam funcionais
- Opção `no_anim` e `prefers-reduced-motion` respeitadas
- Texto crítico não pode virar transparente
- Área segura para overscan
- Modo 720p utilizável
- Nenhum vídeo de tema com áudio
