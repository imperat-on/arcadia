"use client"

import type { FichaJogo, JogoLinha } from "./types"

interface StoreHUDProps {
  destaque: JogoLinha | null
  ficha: FichaJogo | null
  bloqueado: boolean
  semManifesto: boolean
}

// Faixa fina fixa no rodapé, sempre presente. Enquanto o painel expandido
// da grade cuida do trailer e da arte, o HUD carrega a leitura textual do
// jogo em foco — título, gêneros/data, preço e legenda de botões. Assim o
// painel pode viver só de imagem sem depender de mais nada.
export function StoreHUD({ destaque, ficha, bloqueado, semManifesto }: StoreHUDProps) {
  const podeAgir = Boolean(destaque) && !semManifesto && !bloqueado
  return (
    <div className="loja-hud shrink-0 border-t border-white/[0.06] bg-black">
      <div className="flex h-[76px] items-center gap-8 px-12">
        {/* Título + subtítulo (esquerda) */}
        <div className="min-w-0 flex-1">
          {destaque ? (
            <div key={destaque.appid} className="loja-hud-titulo min-w-0">
              <h2 className="truncate text-[22px] font-semibold leading-tight text-white">
                {destaque.title}
              </h2>
              <p className="mt-0.5 truncate text-[12px] text-white/45">
                {[
                  ficha?.generos?.slice(0, 3).join(" · "),
                  ficha?.lancamento,
                  destaque.manifest === false ? "Sem manifesto" : null,
                ]
                  .filter(Boolean)
                  .join(" · ") || " "}
              </p>
            </div>
          ) : (
            <p className="text-[13px] text-white/35">Escolha uma categoria e navegue com os direcionais.</p>
          )}
        </div>

        {/* Preço + Metacritic (meio) */}
        <div className="flex shrink-0 items-center gap-3">
          {ficha?.metacritic ? (
            <span
              className="rounded px-1.5 py-0.5 text-[11px] font-semibold text-black"
              style={{ background: "var(--loja-cor)" }}
              title="Metacritic"
            >
              {ficha.metacritic}
            </span>
          ) : null}
          {ficha?.preco ? (
            <span
              className="text-[18px] font-semibold"
              style={{ color: "var(--loja-cor)" }}
            >
              {ficha.preco}
            </span>
          ) : null}
        </div>

        {/* Legenda de botões (direita) */}
        <div className="flex shrink-0 items-center gap-4 text-[12px] text-white/55">
          <Legenda tecla="A" rotulo="abrir" ativo={Boolean(destaque)} />
          <Legenda tecla="X" rotulo="baixar" ativo={podeAgir} />
          <Legenda tecla="Y" rotulo="adicionar" ativo={podeAgir} />
        </div>
      </div>
    </div>
  )
}

function Legenda({ tecla, rotulo, ativo }: { tecla: string; rotulo: string; ativo: boolean }) {
  return (
    <span className={`flex items-center gap-1.5 transition-opacity ${ativo ? "opacity-100" : "opacity-40"}`}>
      <span className="inline-flex h-5 w-5 items-center justify-center rounded-full border border-white/30 text-[10px] font-bold text-white/85">
        {tecla}
      </span>
      {rotulo}
    </span>
  )
}
