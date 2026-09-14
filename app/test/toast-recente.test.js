"use strict"

// O toast + som de conquista só pode aparecer para desbloqueio RECENTE.
//
// Sintoma real: o app avisava "do nada", com som, uma conquista que o usuário havia
// desbloqueado há meses — item antigo reaparecendo como "não desbloqueado" (store
// reconstruído, item adotado do servidor, passe forçado, troca de conta). O vigia roda
// a cada 15s, então conquista de verdade chega com timestamp de agora; a idade do
// desbloqueio é o que separa um caso do outro.

const test = require("node:test")
const assert = require("node:assert/strict")
const fs = require("node:fs")
const path = require("node:path")

const main = fs.readFileSync(path.join(__dirname, "..", "electron", "main.js"), "utf8")

test("toast de conquista só dispara para desbloqueio recente", () => {
  assert.match(main, /const JANELA_TOAST_S = \d+/, "existe a janela de recência")

  const inicio = main.indexOf("const toastPayload = { ...payload, done, total }")
  const fim = main.indexOf("// Callback de revogo")
  assert.ok(inicio > 0 && fim > inicio, "achei o bloco do aviso")
  const bloco = main.slice(inicio, fim)

  assert.match(
    bloco,
    /const recente = quando > 0 && agora - quando <= JANELA_TOAST_S/,
    "a decisão olha a idade do desbloqueio",
  )
  assert.match(bloco, /if \(recente\) \{/, "o aviso está dentro da condição")

  const posIf = bloco.indexOf("if (recente) {")
  assert.ok(
    bloco.indexOf("showAchievementToast") > posIf,
    "a notificação nativa (toast + som) também está condicionada",
  )
  assert.ok(
    bloco.indexOf('webContents.send("achievement:unlocked"') > posIf,
    "e o aviso do renderer também",
  )
  assert.match(bloco, /toast-suprimido/, "o que foi suprimido fica registrado no log")
})
