"use strict"

const { db, initDb } = require("../src/db")

async function main() {
  await initDb()
  console.log("Migrations aplicadas com sucesso")
}

main()
  .catch((error) => {
    console.error("Falha ao aplicar migrations:", error)
    process.exitCode = 1
  })
  .finally(() => db.end())
