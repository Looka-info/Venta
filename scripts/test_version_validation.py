#!/usr/bin/env python3
"""
Regression test suite for Vanta Version Consistency & CI Release Gate Validation.
Verifies all cases outlined in Milestone 28 Task 3.
"""

import sys
import unittest
from verify_version_consistency import validate_version_consistency, semver_to_release_tag

class TestVersionConsistency(unittest.TestCase):

    def setUp(self):
        self.dummy_repo = "."

    def test_valid_rc2_version(self):
        packages = [
            {"name": "vanta_source", "version": "1.0.0-rc.2"},
            {"name": "vanta_driver", "version": "1.0.0-rc.2"},
            {"name": "vanta_mir", "version": "1.0.0-rc.2"},
        ]
        ok, info = validate_version_consistency(self.dummy_repo, expected_tag="v1.0-RC2", override_packages=packages)
        self.assertTrue(ok)
        self.assertEqual(info["version"], "1.0.0-rc.2")
        self.assertEqual(info["release_tag"], "1.0-RC2")
        self.assertEqual(info["tarball"], "vanta-v1.0-RC2-x86_64-unknown-linux-gnu.tar.gz")

    def test_valid_rc1_version(self):
        packages = [
            {"name": "vanta_source", "version": "1.0.0-rc.1"},
            {"name": "vanta_driver", "version": "1.0.0-rc.1"},
        ]
        ok, info = validate_version_consistency(self.dummy_repo, expected_tag="v1.0-RC1", override_packages=packages)
        self.assertTrue(ok)
        self.assertEqual(info["release_tag"], "1.0-RC1")

    def test_valid_stable_version(self):
        packages = [
            {"name": "vanta_source", "version": "1.0.0"},
            {"name": "vanta_driver", "version": "1.0.0"},
        ]
        ok, info = validate_version_consistency(self.dummy_repo, expected_tag="v1.0.0", override_packages=packages)
        self.assertTrue(ok)
        self.assertEqual(info["release_tag"], "1.0.0")

        # Stable also accepts concise tag 'v1.0'
        ok, info = validate_version_consistency(self.dummy_repo, expected_tag="v1.0", override_packages=packages)
        self.assertTrue(ok)

    def test_reject_unsupported_versions(self):
        invalid_versions = ["0.1.0", "0.9.0", "1.0.0-alpha.1", "1.0.0-beta", "2.0.0", "v1.0.0"]
        for ver in invalid_versions:
            packages = [
                {"name": "vanta_source", "version": ver},
                {"name": "vanta_driver", "version": ver},
            ]
            ok, _ = validate_version_consistency(self.dummy_repo, override_packages=packages)
            self.assertFalse(ok, f"Expected version '{ver}' to be rejected under release policy")

    def test_reject_mismatched_package_versions(self):
        packages = [
            {"name": "vanta_source", "version": "1.0.0-rc.1"},
            {"name": "vanta_driver", "version": "1.0.0-rc.2"},
        ]
        ok, _ = validate_version_consistency(self.dummy_repo, override_packages=packages)
        self.assertFalse(ok, "Expected mismatched versions to be rejected")

    def test_order_independence_when_first_package_is_not_driver(self):
        # Even if packages[0] is not vanta_driver or is some auxiliary crate
        packages = [
            {"name": "vanta_auxiliary_util", "version": "1.0.0-rc.2"},
            {"name": "vanta_lexer", "version": "1.0.0-rc.2"},
            {"name": "vanta_driver", "version": "1.0.0-rc.2"},
        ]
        ok, info = validate_version_consistency(self.dummy_repo, override_packages=packages)
        self.assertTrue(ok)
        self.assertEqual(info["version"], "1.0.0-rc.2")

    def test_tag_mismatch_rejection(self):
        packages = [
            {"name": "vanta_driver", "version": "1.0.0-rc.2"},
        ]
        # Tag says RC1 but version is RC2
        ok, _ = validate_version_consistency(self.dummy_repo, expected_tag="v1.0-RC1", override_packages=packages)
        self.assertFalse(ok, "Expected tag v1.0-RC1 to mismatch version 1.0.0-rc.2")

        # Tag says v1.0.0 but version is RC2
        ok, _ = validate_version_consistency(self.dummy_repo, expected_tag="v1.0.0", override_packages=packages)
        self.assertFalse(ok, "Expected tag v1.0.0 to mismatch version 1.0.0-rc.2")

    def test_current_workspace_validation(self):
        # Validate actual workspace files
        import os
        repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
        ok, info = validate_version_consistency(repo_root)
        self.assertTrue(ok, "Current workspace must pass version consistency check")
        self.assertEqual(info["version"], "1.0.0-rc.2")

if __name__ == "__main__":
    unittest.main()
