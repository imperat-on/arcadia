// Registry de runners (store managers) — mesma interface para todos:
// { status(), login(), library(), callRunner(args, opts) }.
// gog/nile entram na Fase 2 com a mesma assinatura.

const legendary = require("./legendary")

module.exports = {
  legendary,
  // gog: require("./gog"),   // Fase 2
  // nile: require("./nile"), // Fase 2
}
