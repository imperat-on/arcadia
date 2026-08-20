# Deploy do Arcadia Server

O backend atual usa Node.js, Express e PostgreSQL. O SQLite em `server/data` e legado e serve apenas para migracao.

## Configuracao

```bash
cd /home/zes/.local/share/arcadia/server
cp .env.example .env
openssl rand -hex 32
```

Defina no `.env` um `JWT_SECRET` forte, `DATABASE_URL`, `TEST_DATABASE_URL` e `DATA_DIR`. Nunca publique o arquivo `.env` nem o inclua em backups acessiveis.

## Banco e inicializacao

Crie o banco e o usuario PostgreSQL e deixe `DATABASE_URL` disponível para o
processo. O próprio servidor aplica a baseline e as migrations versionadas:

```bash
npm install
npm run migrate:postgres
npm run verify:postgres
npm test             # requer TEST_DATABASE_URL e PostgreSQL de teste
npm start
curl http://127.0.0.1:3000/health
```

`server/sql/schema.sql` é a baseline v1 mantida para instalações antigas;
alterações futuras entram em `server/sql/migrations/` e são aplicadas pelo
runner com lock e checksum.

## systemd e Tailscale

O arquivo `server/systemd/arcadia-server.service` fornece um exemplo de unidade systemd. Ajuste `User`, `WorkingDirectory` e `EnvironmentFile` para a maquina de deploy. Exponha o servico apenas por HTTPS, por exemplo via Tailscale Funnel, e configure `ARCADIA_API_URL` no cliente Electron para a URL publica (os aliases `ARCADIA_SUPABASE_URL` e `SUPABASE_URL` permanecem compatíveis).

Mantenha PostgreSQL e os diretorios `avatars`, `backgrounds` e `banners` fora da area publica. Use `server/scripts/backup-postgres.sh` para gerar backups do banco e storage, teste a restauracao e mantenha permissoes restritas.

## Operacao

- confirme `/health` apos cada deploy;
- acompanhe logs do systemd e falhas de catalogo;
- aplique migracoes antes de iniciar uma versao que dependa de novas colunas;
- nao use o segredo de exemplo em producao;
- prefira HTTPS para evitar enviar tokens em claro.
