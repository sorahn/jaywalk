"""Portable metadata tests; real signing/Gatekeeper are separate acceptance checks."""
import importlib.util
from pathlib import Path
import tempfile
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

    def test_nested_intel_helper_and_intel_launcher_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = Path(temporary) / "Jaywalk.app"
            binaries = bundle / "Contents/MacOS"
            binaries.mkdir(parents=True)
            for name in ("jaywalk", "jw"):
                (binaries / name).write_bytes(bytes.fromhex("cffaedfe"))
            helper = bundle / "Contents/Frameworks/Helper"
            helper.parent.mkdir()
            helper.write_bytes(bytes.fromhex("cafebabe"))
            architectures = {"jaywalk": "arm64", "jw": "arm64", "Helper": "x86_64"}
            with patch.object(release, "run", side_effect=lambda *args: architectures[Path(args[-1]).name]):
                with self.assertRaisesRegex(ValueError, "does not support arm64.*Helper"):
                    release.verify_bundle_architecture(bundle)
                architectures["Helper"] = "x86_64 arm64"
                release.verify_bundle_architecture(bundle)
                architectures["jw"] = "x86_64"
                with self.assertRaisesRegex(ValueError, "arm64 jw"):
                    release.verify_bundle_architecture(bundle)

    def test_local_intel_bundles_require_native_binaries_and_helpers(self):
        with tempfile.TemporaryDirectory() as temporary:
            bundle = Path(temporary) / "Jaywalk.app"
            binaries = bundle / "Contents/MacOS"
            binaries.mkdir(parents=True)
            for name in ("jaywalk", "jw", "helper"):
                (binaries / name).write_bytes(bytes.fromhex("cffaedfe"))
            architectures = {"jaywalk": "x86_64", "jw": "x86_64", "helper": "arm64 x86_64"}
            with patch.object(release, "run", side_effect=lambda *args: architectures[Path(args[-1]).name]):
                release.verify_bundle_architecture(bundle, "x86_64")
                # The default release gate must still reject Intel binaries.
                with self.assertRaisesRegex(ValueError, "arm64 jaywalk"):
                    release.verify_bundle_architecture(bundle)
                architectures["helper"] = "arm64"
                with self.assertRaisesRegex(ValueError, "does not support x86_64.*helper"):
                    release.verify_bundle_architecture(bundle, "x86_64")
                architectures["jw"] = "arm64"
                with self.assertRaisesRegex(ValueError, "x86_64 jw"):
                    release.verify_bundle_architecture(bundle, "x86_64")

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
