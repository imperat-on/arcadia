"use client"

import { useEffect, useRef, useState } from "react";
import type { Profile } from "../../global"
import { useI18n } from "../../i18n/I18nContext";

export interface ProfileSelectProps {
  profiles: Profile[];
  onSelect: (index: number) => void;
  onAdd: () => void;
  onEdit?: (index: number) => void;
  onDelete?: (index: number) => void;
}

const initialOf = (name?: string) =>
  (name?.trim()?.[0] ?? "?").toUpperCase();

export default function ProfileSelect({
  profiles,
  onSelect,
  onAdd,
  onEdit,
  onDelete,
}: ProfileSelectProps) {
  const { t } = useI18n();
  const [focus, setFocus] = useState(0);
  const [menuOpen, setMenuOpen] = useState(false);
  const total = profiles.length + 1; // +1 = "add"
  const btnRefs = useRef<Array<HTMLButtonElement | null>>([]);

  useEffect(() => {
    btnRefs.current[focus]?.focus();
  }, [focus]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight") {
        e.preventDefault();
        setMenuOpen(false);
        setFocus((f) => Math.min(total - 1, f + 1));
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        setMenuOpen(false);
        setFocus((f) => Math.max(0, f - 1));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [total]);

  // background do perfil focado (só quando é perfil, não o "add")
  const focusedProfile =
    focus < profiles.length ? profiles[focus] : undefined;

  const activate = (i: number) => {
    if (i === profiles.length) onAdd();
    else onSelect(i);
  };

  return (
    <div className="ps5-profile-root relative flex min-h-screen w-full flex-col items-center justify-center overflow-hidden bg-[#070a12] text-white">
      {/* Fundo da seleção: SEMPRE o azul-noite estático + glow quente.
          O background animado do perfil só aparece dentro do sistema
          (aba Meu Perfil), nunca aqui na entrada. */}
      <div className="pointer-events-none absolute inset-0">
        <div className="ps5-glow absolute inset-0" />
      </div>

      <h1
        className="relative z-10 mb-[clamp(3rem,6vh,6rem)] font-semibold tracking-tight text-white"
        style={{ fontSize: "clamp(2rem, 3.4vw, 4rem)" }}
      >
        {t("profile.quem_esta_jogando")}
      </h1>

      <div
        className="relative z-10 flex w-full items-start justify-center gap-[clamp(1rem,2.5vw,3rem)] px-8"
        role="radiogroup"
        aria-label={t("profile.selecionar_perfil")}
      >
        {profiles.map((p, i) => {
          const isFocus = focus === i;
          return (
            <div
              key={i}
              className="ps5-slot relative flex flex-col items-center"
              data-focus={isFocus}
            >
              <button
                ref={(el) => {
                  btnRefs.current[i] = el;
                }}
                type="button"
                role="radio"
                aria-checked={isFocus}
                aria-label={t("profile.entrar_como", { name: p.name ?? t("profile.jogador") })}
                onMouseEnter={() => setFocus(i)}
                onFocus={() => setFocus(i)}
                onClick={() => onSelect(i)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onSelect(i);
                  }
                }}
                className="ps5-avatar relative grid place-items-center overflow-hidden rounded-full outline-none transition-all duration-300"
              >
                {p.avatar ? (
                  <img
                    src={p.avatar}
                    alt=""
                    className="h-full w-full object-cover"
                    draggable={false}
                  />
                ) : (
                  <span
                    className="grid h-full w-full place-items-center font-semibold text-white"
                    style={{
                      background:
                        "linear-gradient(135deg, var(--color-ps-blue), #003791)",
                      fontSize: "clamp(2.5rem, 3vw, 4rem)",
                    }}
                  >
                    {initialOf(p.name)}
                  </span>
                )}
              </button>

              <div className="mt-4 flex items-center gap-2">
                <span
                  className="font-medium text-white/90"
                  style={{ fontSize: "clamp(0.95rem, 1vw, 1.2rem)" }}
                >
                  {p.name ?? t("profile.sem_nome")}
                </span>
                {p.owner && (
                  <span className="rounded bg-white/10 px-1.5 py-0.5 text-[0.65rem] uppercase tracking-wider text-white/70">
                    {t("profile.dono")}
                  </span>
                )}
              </div>

              {isFocus && (onEdit || onDelete) && (
                <div className="absolute right-1 top-1">
                  <button
                    type="button"
                    aria-label={t("profile.acoes_perfil")}
                    onClick={(e) => {
                      e.stopPropagation();
                      setMenuOpen((v) => !v);
                    }}
                    className="grid h-8 w-8 place-items-center rounded-full bg-black/60 text-white/80 opacity-0 transition-opacity duration-200 hover:bg-black/80 focus-visible:opacity-100 group-hover:opacity-100 [.ps5-slot[data-focus=true]_&]:opacity-100"
                  >
                    <span className="text-lg leading-none">⋯</span>
                  </button>
                  {menuOpen && (
                    <div
                      role="menu"
                      className="absolute right-0 top-10 min-w-[9rem] overflow-hidden rounded-md border border-white/10 bg-[var(--color-ps-surface2)] shadow-2xl"
                    >
                      {onEdit && (
                        <button
                          type="button"
                          role="menuitem"
                          className="block w-full px-4 py-2 text-left text-sm text-white/90 hover:bg-white/10"
                          onClick={(e) => {
                            e.stopPropagation();
                            setMenuOpen(false);
                            onEdit(i);
                          }}
                        >
                          {t("profile.editar_perfil")}
                        </button>
                      )}
                      {onDelete && (
                        <button
                          type="button"
                          role="menuitem"
                          className="block w-full px-4 py-2 text-left text-sm text-red-400 hover:bg-white/10"
                          onClick={(e) => {
                            e.stopPropagation();
                            setMenuOpen(false);
                            onDelete(i);
                          }}
                        >
                          {t("profile.excluir")}
                        </button>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}

        {/* Add card */}
        <div
          className="ps5-slot relative flex flex-col items-center"
          data-focus={focus === profiles.length}
        >
          <button
            ref={(el) => {
              btnRefs.current[profiles.length] = el;
            }}
            type="button"
            aria-label={t("profile.adicionar_usuario")}
            onMouseEnter={() => setFocus(profiles.length)}
            onFocus={() => setFocus(profiles.length)}
            onClick={() => activate(profiles.length)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onAdd();
              }
            }}
            className="ps5-avatar ps5-avatar--add grid place-items-center rounded-full outline-none transition-all duration-300"
          >
            <span
              className="font-thin text-white/70"
              style={{ fontSize: "clamp(3rem, 4vw, 5rem)" }}
            >
              +
            </span>
          </button>
          <div className="mt-4">
            <span
              className="font-medium text-white/90"
              style={{ fontSize: "clamp(0.95rem, 1vw, 1.2rem)" }}
            >
              {t("profile.adicionar_usuario")}
            </span>
          </div>
        </div>
      </div>

      <p
        className="relative z-10 mt-[clamp(2rem,5vh,4rem)] text-white/60 transition-opacity duration-300"
        style={{
          fontSize: "clamp(0.85rem, 0.95vw, 1.05rem)",
          opacity: focus < profiles.length ? 1 : 0,
        }}
      >
        {t("profile.pressione")} <kbd className="mx-1 rounded bg-white/10 px-2 py-0.5">X</kbd> {t("profile.para_entrar")}
      </p>
    </div>
  );
}
