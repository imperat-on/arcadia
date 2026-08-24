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
runner com lock e checksum. A migration `0003_community.sql` cria reviews
versionadas, coleções, itens, reports e moderadores sem apagar dados legados.

Para um host que não possui checkout Git, use uma cópia explícita e reversível:

```bash
stamp=$(date -u +%Y%m%d-%H%M%S)
backup="backups/deploy-$stamp"
mkdir -p "$backup/files"
pg_dump --format=custom --file="$backup/postgres.dump" "$DATABASE_URL"
cp -a src/server.js src/db.js package.json "$backup/files/"
# copie o artefato para uma pasta temporária, valide `node --check` e só então
# substitua os arquivos; reinicie a unidade sem reutilizar senhas compartilhadas.
node --check src/server.js
node --check src/community-routes.js
node --check src/community-validation.js
node -e 'require("./src/db").initDb().then(() => process.exit(0))'
```

Após a troca, confirme `/health`, `/ready`, a tabela `schema_migrations` e os
endpoints públicos de catálogo/comunidade. Se a migration ou o healthcheck
falhar, pare o serviço, restaure os arquivos de `backup/files`, reinicie e
investigue antes de tentar novamente.

## systemd e Tailscale

O arquivo `server/systemd/arcadia-server.service` fornece um exemplo de unidade systemd. Ajuste `User`, `WorkingDirectory` e `EnvironmentFile` para a maquina de deploy. Exponha o servico apenas por HTTPS, por exemplo via Tailscale Funnel, e configure `ARCADIA_API_URL` no cliente Electron para a URL publica (os aliases `ARCADIA_SUPABASE_URL` e `SUPABASE_URL` permanecem compatíveis).

Mantenha PostgreSQL e os diretorios `avatars`, `backgrounds` e `banners` fora da area publica. Use `server/scripts/backup-postgres.sh` para gerar backups do banco e storage, teste a restauracao e mantenha permissoes restritas.

## Operacao

- confirme `/health` apos cada deploy;
- acompanhe logs do systemd e falhas de catalogo;
- aplique migracoes antes de iniciar uma versao que dependa de novas colunas;
- nao use o segredo de exemplo em producao;
- prefira HTTPS para evitar enviar tokens em claro.
