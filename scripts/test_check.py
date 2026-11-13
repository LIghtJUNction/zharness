"""Feature policy checks independent of compiler/network availability."""

import unittest
from check import FEATURES, feature_args


class FeatureTests(unittest.TestCase):
    def test_all_explicit_feature_combinations(self):
        self.assertEqual(set(FEATURES), {(), ("std",), ("wasm",), ("std", "wasm")})

    def test_core_does_not_accidentally_enable_default_std(self):
        self.assertEqual(feature_args(()), ["--no-default-features"])

    def test_wasm_does_not_require_std(self):
        self.assertEqual(feature_args(("wasm",)), ["--no-default-features", "--features", "wasm"])

    def test_combined_features_are_a_single_cargo_argument(self):
        self.assertEqual(feature_args(("std", "wasm")),
                         ["--no-default-features", "--features", "std,wasm"])


if __name__ == "__main__":
    unittest.main()
