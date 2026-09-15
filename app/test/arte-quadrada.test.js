const test = require('node:test')
const assert = require('node:assert/strict')
const { classificar, ordenar } = require('../electron/arte-quadrada')

test('classificar separa quadrado, promocional e retrato', () => {
  assert.equal(classificar({ url: 'https://a/1.jpg', largura: 512, altura: 512 }), 'quadrado')
  assert.equal(classificar({ url: 'https://a/2.jpg', largura: 3840, altura: 1240 }), 'preencher')
  assert.equal(classificar({ url: 'https://a/3.jpg', largura: 600, altura: 900 }), 'capa')
})

test('classificar descarta o que nao serve para o quadrado', () => {
  assert.equal(classificar({ url: 'http://a/1.jpg', largura: 512, altura: 512 }), null, 'sem https')
  assert.equal(classificar({ url: 'https://a/1.jpg', largura: 128, altura: 128 }), null, 'pequeno demais')
  assert.equal(classificar({ url: 'https://a/1.gif', largura: 512, altura: 512, animado: true }), null, 'animado')
  assert.equal(classificar({ url: 'https://a/1.jpg', largura: 0, altura: 0 }), null, 'dimensao desconhecida')
  assert.equal(classificar(null), null)
})

test('ordenar prefere quadrado, depois promocional, depois retrato, sem repetir url', () => {
  const lista = ordenar([
    { url: 'https://a/capa.jpg', largura: 600, altura: 900 },
    { url: 'https://a/hero.jpg', largura: 3840, altura: 1240 },
    { url: 'https://a/quadrado.jpg', largura: 512, altura: 512 },
    { url: 'https://a/hero.jpg', largura: 3840, altura: 1240 },
    { url: 'https://a/lixo.gif', largura: 512, altura: 512, animado: true },
  ])
  assert.deepEqual(lista.map((c) => c.url), [
    'https://a/quadrado.jpg',
    'https://a/hero.jpg',
    'https://a/capa.jpg',
  ])
  assert.deepEqual(lista.map((c) => c.ajuste), ['quadrado', 'preencher', 'capa'])
})

test('ordenar, dentro da mesma classe, prefere a imagem maior', () => {
  const lista = ordenar([
    { url: 'https://a/512.jpg', largura: 512, altura: 512 },
    { url: 'https://a/1024.jpg', largura: 1024, altura: 1024 },
  ])
  assert.deepEqual(lista.map((c) => c.url), ['https://a/1024.jpg', 'https://a/512.jpg'])
})
