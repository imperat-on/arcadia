# Design — Perfil Único (conta online como identidade) 2026-08-05

Status: aprovado pelo usuário (fluxo brainstorm)
Problema: sidebar mostra a conta online (username) e a Home mostra o perfil
local (name/avatar) — duas identidades.

## Decisões (aprovadas)
- Quando logado, a identidade é ÚNICA e vem do servidor.
- Nome de exibição editável = `display_name` na tabela profiles (sincroniza
  entre máquinas). Fallback: username.
- Sincronizam pro servidor: display_name, summary, country, city, showcase.
- Avatar: já sobe pro Storage (feito). Fundo de perfil: fica LOCAL.
- Limites free: textos ~1KB/usuário cabem nos 50k MAU/500MB; imagens pesadas
  ficam fora do Postgres (Storage) — fundo local evita estourar.

## Mecânica
- `profiles` ganha colunas: display_name, summary, country, city, showcase jsonb.
- RLS existente (update using auth.uid()=id) já permite o update direto.
- `myProfile()` passa a retornar os campos novos; novo `updateProfile(campos)`
  grava só os campos permitidos (whitelist).
- Renderer: AccountContext mantém `perfil` estendido + `updatePerfil()`.
- ProfileBridge (dentro do provider): quando logado, espelha o perfil online
  no perfil local em memória (name=display_name||username, avatar=avatar_url,
  summary/country/city/showcase); ao deslogar, restaura o original.
- EditProfile (logado): salva local + sincroniza os campos aprovados.
- Amigos: busca/lista passam a incluir display_name (fallback username).

## Fora de escopo
- realName e background ficam locais.
- Badge "Owner" permanece como está (local).
