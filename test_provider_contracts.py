"""Testes do contrato interno de providers."""

import unittest
from pathlib import Path

from indexers.contracts import ProviderContext, ProviderResult, execute_provider


class ProviderContractTests(unittest.TestCase):
    def setUp(self):
        self.context = ProviderContext(data_dir=Path("/tmp/arcadia-contract"), config={"x": 1})

    def test_success_filters_invalid_games_and_preserves_warning(self):
        result = execute_provider("fake", lambda: [{"id": "fake:1"}, "invalid"], self.context)
        self.assertTrue(result.ok)
        self.assertEqual(["fake:1"], [game["id"] for game in result.games])
        self.assertEqual(("entrada de jogo inválida descartada",), result.warnings)
        self.assertEqual("fake", result.as_dict()["provider"])

    def test_failure_isolated_and_measured(self):
        result = execute_provider("broken", lambda: (_ for _ in ()).throw(RuntimeError("offline")), self.context)
        self.assertFalse(result.ok)
        self.assertEqual(("offline",), result.errors)
        self.assertGreaterEqual(result.elapsed_ms, 0)

    def test_result_rejects_non_list_without_raising(self):
        result = execute_provider("bad", lambda: {"id": "bad:1"}, self.context)
        self.assertFalse(result.ok)
        self.assertEqual(("resultado não é uma lista",), result.errors)

    def test_context_is_immutable_contract(self):
        with self.assertRaises(Exception):
            self.context.language = "pt-BR"


if __name__ == "__main__":
    unittest.main()
