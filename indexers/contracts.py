"""Contratos internos para providers do indexador.

Os providers continuam podendo ser funções simples e determinísticas. Este
módulo define a fronteira comum para que descoberta local, rede e diagnóstico
sejam evoluídos sem alterar o envelope persistido da biblioteca.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from time import monotonic
from typing import Any, Callable, Mapping


@dataclass(frozen=True)
class ProviderContext:
    """Dependências e opções estáveis compartilhadas por um provider."""

    data_dir: Path
    config: Mapping[str, Any] = field(default_factory=dict)
    language: str = "en-US"
    network_enabled: bool = True


@dataclass(frozen=True)
class ProviderResult:
    """Resultado normalizado de uma execução, sem escrever em disco."""

    provider: str
    games: tuple[dict[str, Any], ...] = ()
    warnings: tuple[str, ...] = ()
    errors: tuple[str, ...] = ()
    elapsed_ms: int = 0

    @property
    def ok(self) -> bool:
        return not self.errors

    @classmethod
    def success(
        cls,
        provider: str,
        games: list[dict[str, Any]] | tuple[dict[str, Any], ...] = (),
        warnings: list[str] | tuple[str, ...] = (),
        elapsed_ms: int = 0,
    ) -> "ProviderResult":
        return cls(
            provider=str(provider),
            games=tuple(games),
            warnings=tuple(str(item) for item in warnings),
            elapsed_ms=max(0, int(elapsed_ms)),
        )

    @classmethod
    def failure(cls, provider: str, error: str, elapsed_ms: int = 0) -> "ProviderResult":
        return cls(
            provider=str(provider),
            errors=(str(error),),
            elapsed_ms=max(0, int(elapsed_ms)),
        )

    def as_dict(self) -> dict[str, Any]:
        return {
            "provider": self.provider,
            "games": list(self.games),
            "warnings": list(self.warnings),
            "errors": list(self.errors),
            "elapsed_ms": self.elapsed_ms,
        }


def execute_provider(
    provider: str,
    function: Callable[[], list[dict[str, Any]]],
    _context: ProviderContext,
) -> ProviderResult:
    """Executa uma função legada na fronteira e captura falhas isoladamente."""

    started = monotonic()
    try:
        games = function()
        if not isinstance(games, list):
            return ProviderResult.failure(
                provider,
                "resultado não é uma lista",
                int((monotonic() - started) * 1000),
            )
        valid = [game for game in games if isinstance(game, dict)]
        warnings = () if len(valid) == len(games) else ("entrada de jogo inválida descartada",)
        return ProviderResult.success(
            provider,
            valid,
            warnings,
            int((monotonic() - started) * 1000),
        )
    except Exception as exc:  # o caller decide como exibir/persistir o diagnóstico
        return ProviderResult.failure(
            provider,
            str(exc),
            int((monotonic() - started) * 1000),
        )
