# Design — Perfil de Amigo + Tela de Amigos turbinada (2026-08-05)

Status: aprovado pelo usuário (fluxo brainstorm)
Escopo: visual da tela Amigos + perfil público de amigo + ações por estado

## Objetivo
- Deixar a tela Amigos com a identidade do launcher (dark, neon #0072ce/#00a8ff, glass, micro-animações CSS).
- Permitir entrar no PERFIL de um amigo e ver as conquistas recentes dele.
- Ações contextuais por estado da relação (busca, pendente, aceito).

## Backend (Supabase — SQL de migração)

### 1. RPC `friend_achievements(p_friend uuid)`
Security definer (burla RLS self-only) que SÓ retorna conquistas se os dois
forem amigos (friendships status='accepted' em qualquer direção).
Retorna: appid, apiname, unlocked_at, updated_at — limit 30, mais recentes.
Grant: to authenticated.

### 2. Policy de delete de amigo aceito
Hoje só existe delete do requester em pedido pending. Adiciona:
`friends_delete_accepted` — delete to authenticated
using (auth.uid() in (user_a, user_b) and status = 'accepted')
(PostgreSQL OR de políticas: as duas continuam valendo.)

### 3. Lista de amigos (código, sem SQL)
`friendsList()` passa a incluir: avatar_url (do profiles) e created_at
(da friendships) para o card "Amigos desde".

## Renderer

### FriendsView (redesign)
- Cards: avatar circular com letra + cor derivada do username (hash → HSL),
  username, ação por estado; hover com glow azul; fade-up na entrada.
- Seções: Amigos (lista) / Recebidos (badge) / Enviados / Resultados da busca.
- Estados vazios bonitos (ícone + texto).
- Clique no amigo aceito → abre FriendProfileView.

### FriendProfileView (novo)
- Header: avatar gigante, username, "Amigos desde DD/MM/AAAA".
- Grid de conquistas recentes do amigo (RPC): apiname + data; se o appid
  existir no achievements.json local, usa título/ícone reais (enriquecimento).
- Botão Remover amigo (com confirmação inline) + voltar.

### Ações por estado
- Busca: [Adicionar] / [Pendente]
- Recebido: [Aceitar] / [Recusar]
- Enviado: [Cancelar]
- Amigo: [Ver perfil] · [Remover]

## Erros
- friend_achievements sem amizade → RPC devolve vazio (não vaza dados).
- Remover amigo usa a policy nova; falha → mensagem visível (padrão do app).
