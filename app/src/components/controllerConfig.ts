// Cache da configuração de controle, fora do React.
//
// Os laços de gamepad (useGamepadNav) rodam a 60fps e precisam ler a zona morta
// e o liga/desliga a CADA frame: ir por IPC ou pelo contexto do React custaria
// uma promessa/re-render por frame. Por isso a config vive num módulo, semeada
// quando o launcher carrega o config e sempre que a UI salva.

export const DEADZONE_MIN = 0.15
export const DEADZONE_MAX = 0.85
export const DEADZONE_PADRAO = 0.6

export interface ControllerConfig {
  enable_controller_navigation: boolean
  controller_deadzone: number
}

let cache: ControllerConfig = {
  enable_controller_navigation: true,
  controller_deadzone: DEADZONE_PADRAO,
}

/** Traz a zona morta para a faixa do slider (o config pode ter valor legado). */
export function clampDeadzone(valor: number): number {
  if (!Number.isFinite(valor)) return DEADZONE_PADRAO
  return Math.min(DEADZONE_MAX, Math.max(DEADZONE_MIN, valor))
}

export function setControllerConfig(patch: Partial<ControllerConfig>): void {
  if (patch.enable_controller_navigation !== undefined) {
    // Qualquer coisa que não seja `false` explícito mantém a navegação ligada:
    // o padrão do produto é ligado, e um config ausente não pode desligá-la.
    cache.enable_controller_navigation = patch.enable_controller_navigation !== false
  }
  if (patch.controller_deadzone !== undefined) {
    cache.controller_deadzone = clampDeadzone(Number(patch.controller_deadzone))
  }
}

export function getControllerConfig(): ControllerConfig {
  return cache
}

/** Atalho para os laços: leem a cada frame, sem desestruturar o objeto. */
export function navegacaoControleAtiva(): boolean {
  return cache.enable_controller_navigation
}

export function deadzoneAtual(): number {
  return cache.controller_deadzone
}
