// Registry de runners (store managers) — mesma interface para todos:
// { status(), login(), library(), callRunner(args, opts) }.

const legendary = require("./legendary")

module.exports = {
  legendary,
}
