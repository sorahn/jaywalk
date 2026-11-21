"""Portable metadata tests; real signing/Gatekeeper are separate acceptance checks."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("release-macos.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

class ReleaseTests(unittest.TestCase):
    def test_tag_must_match_source(self):
        release.validate_tag("v" + release.version())
        for tag in ("main", "v1999.0.0", "" + release.version()):
            with self.assertRaises(ValueError):
                release.validate_tag(tag)

    def test_injected_app_version_is_independent_of_cargo(self):
        with patch.dict("os.environ", {"JAYWALK_RELEASE_VERSION": "2026.12.7"}):
            self.assertEqual(release.version(), "2026.12.7")
            release.validate_tag("v2026.12.7")
            with self.assertRaises(ValueError):
                release.validate_tag("v0.0.0")

    def test_cask_rejects_unsigned_and_invalid_inputs(self):
        data = {"official": True, "architecture": "arm64", "version": "1.2.3", "artifacts": [{"filename": "Jaywalk-1.2.3-macos-arm64.dmg", "sha256": "a" * 64}]}
        result = release.cask(data)
        self.assertIn("auto_updates true", result)
        self.assertIn('binary "#{appdir}/Jaywalk.app/Contents/MacOS/jw"', result)
        self.assertNotIn("zap", result)
        for key, value in (("official", False), ("architecture", "x86_64"), ("version", '1.2.3"\nmalicious')):
            with self.assertRaises(ValueError):
                release.cask({**data, key: value})

if __name__ == "__main__":
    unittest.main()
