"use strict"

// Catraca do parser de VDF + da leitura das contas/horas da Steam.
//
// O formato real está nos testes como amostra (estrutura aninhada de verdade,
// como nos arquivos da Valve): regex sobre `{...}` falha justamente aqui, porque
// para no primeiro fecha-chave interno — foi o que não achou o `MostRecent`.

const test = require("node:test")
const assert = require("node:assert/strict")

const {
  parseVDF,
  contasDoLoginUsers,
  contaAtivaDoLoginUsers,
  horasDoLocalConfig,
  steamidParaContaId,
  contaIdParaSteamid,
} = require("../electron/vdf")

const LOGIN_USERS = `"users"
{
\t"76561198000010132"
\t{
\t\t"AccountName"\t\t"conta1"
\t\t"PersonaName"\t\t"NEGO EJACULADO"
\t\t"RememberPassword"\t\t"1"
\t\t"MostRecent"\t\t"0"
\t\t"Timestamp"\t\t"1700000000"
\t}
\t"76561198000004541"
\t{
\t\t"AccountName"\t\t"conta2"
\t\t"PersonaName"\t\t"Kk"
\t\t"RememberPassword"\t\t"1"
\t\t"MostRecent"\t\t"1"
\t\t"Timestamp"\t\t"1800000000"
\t}
}
`

const LOCAL_CONFIG = `"UserLocalConfigStore"
{
\t"Software"
\t{
\t\t"Valve"
\t\t{
\t\t\t"Steam"
\t\t\t{
\t\t\t\t"apps"
\t\t\t\t{
\t\t\t\t\t"990080"
\t\t\t\t\t{
\t\t\t\t\t\t"LastPlayed"\t\t"1750000000"
\t\t\t\t\t\t"Playtime"\t\t"4907"
\t\t\t\t\t\t"Playtime2wks"\t\t"0"
\t\t\t\t\t}
\t\t\t\t\t"1091500"
\t\t\t\t\t{
\t\t\t\t\t\t"Playtime"\t\t"120"
\t\t\t\t\t}
\t\t\t\t\t"0"
\t\t\t\t\t{
\t\t\t\t\t\t"Playtime"\t\t"999"
\t\t\t\t\t}
\t\t\t\t}
\t\t\t}
\t\t}
\t}
}
`

test("parseVDF lê aninhamento, comentários e escapes", () => {
  const d = parseVDF(`// comentario
"raiz"
{
\t"a" "1"
\t"b" { "c" "dois" }
\t"quoted" "diz \\"oi\\""
}
`)
  assert.equal(d.raiz.a, "1")
  assert.equal(d.raiz.b.c, "dois")
  assert.equal(d.raiz.quoted, 'diz "oi"')
})

test("chave repetida vira lista (como na leitura da Valve)", () => {
  const d = parseVDF(`"r" { "k" "1" "k" "2" }`)
  assert.deepEqual(d.r.k, ["1", "2"])
})

test("loginusers: contas, persona e quem está ativo", () => {
  const contas = contasDoLoginUsers(LOGIN_USERS)
  assert.equal(contas.length, 2)
  const ativa = contaAtivaDoLoginUsers(LOGIN_USERS)
  assert.equal(ativa.steamid, "76561198000004541")
  assert.equal(ativa.persona, "Kk")
  assert.equal(contas.find((c) => c.persona === "NEGO EJACULADO").maisRecente, false)
})

test("localconfig: horas por appid, ignorando lixo e zero", () => {
  const horas = horasDoLocalConfig(LOCAL_CONFIG)
  assert.equal(horas["990080"], 4907, "Hogwarts Legacy: 4907 min = 81h47")
  assert.equal(horas["1091500"], 120)
  assert.ok(!("0" in horas), "appid 0 (entrada da própria Steam) fica fora")
})

test("conversão steamid64 <-> pasta de userdata", () => {
  assert.equal(steamidParaContaId("76561198000004541"), "39738813")
  assert.equal(contaIdParaSteamid("39738813"), "76561198000004541")
  assert.equal(steamidParaContaId("lixo"), "")
  assert.equal(contaIdParaSteamid("lixo"), "")
})

test("VDF inválido não estoura em cima do usuário: erro claro", () => {
  assert.throws(() => parseVDF(`"a" sem aspas`), /VDF inválido/)
})
