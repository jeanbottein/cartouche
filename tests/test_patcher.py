"""Unit tests for lib/patcher.py — calculate_crc32() and check_file_status()."""
import zlib

import pytest

from lib.patcher import calculate_crc32, check_file_status


# ── calculate_crc32 ───────────────────────────────────────────────────────

class TestCalculateCrc32:
    def test_known_bytes_produce_expected_crc(self, tmp_path):
        f = tmp_path / "data.bin"
        f.write_bytes(b"hello")
        expected = zlib.crc32(b"hello") & 0xFFFFFFFF
        assert calculate_crc32(str(f)) == expected

    def test_empty_file_crc(self, tmp_path):
        f = tmp_path / "empty.bin"
        f.write_bytes(b"")
        expected = zlib.crc32(b"") & 0xFFFFFFFF
        assert calculate_crc32(str(f)) == expected

    def test_result_is_unsigned_32bit(self, tmp_path):
        f = tmp_path / "data.bin"
        f.write_bytes(b"test data")
        result = calculate_crc32(str(f))
        assert 0 <= result <= 0xFFFFFFFF

    def test_different_content_different_crc(self, tmp_path):
        f1 = tmp_path / "a.bin"
        f2 = tmp_path / "b.bin"
        f1.write_bytes(b"content_a")
        f2.write_bytes(b"content_b")
        assert calculate_crc32(str(f1)) != calculate_crc32(str(f2))


# ── check_file_status ─────────────────────────────────────────────────────

class TestCheckFileStatus:
    def _write_file(self, tmp_path, content):
        f = tmp_path / "target.bin"
        f.write_bytes(content)
        return str(f)

    def _hex_crc(self, content):
        crc = zlib.crc32(content) & 0xFFFFFFFF
        return f"{crc:08X}"

    def test_returns_ready_when_crc_matches_target(self, tmp_path):
        content = b"original content"
        path = self._write_file(tmp_path, content)
        status = check_file_status(path, self._hex_crc(content), None)
        assert status == "ready"

    def test_returns_already_patched_when_crc_matches_patched(self, tmp_path):
        content = b"patched content"
        path = self._write_file(tmp_path, content)
        status = check_file_status(path, "AAAAAAAA", self._hex_crc(content))
        assert status == "already_patched"

    def test_patched_crc_checked_before_target_crc(self, tmp_path):
        """patched_crc32 takes priority over target_crc32."""
        content = b"data"
        crc = self._hex_crc(content)
        path = self._write_file(tmp_path, content)
        # Both match: patched should win
        status = check_file_status(path, crc, crc)
        assert status == "already_patched"

    def test_returns_mismatch_when_neither_crc_matches(self, tmp_path):
        content = b"unexpected content"
        path = self._write_file(tmp_path, content)
        status = check_file_status(path, "00000001", "00000002")
        assert status == "mismatch"

    def test_returns_ready_when_no_target_crc(self, tmp_path):
        """When target_crc32 is None/empty, any file is considered ready."""
        content = b"anything"
        path = self._write_file(tmp_path, content)
        status = check_file_status(path, None, None)
        assert status == "ready"

    def test_status_strings_are_expected_values(self, tmp_path):
        """Documents the string-based status contract (not Enum — known issue)."""
        content = b"data"
        path = self._write_file(tmp_path, content)
        status = check_file_status(path, None, None)
        assert status in ("ready", "already_patched", "mismatch")

    def test_invalid_hex_target_crc_returns_mismatch_not_crash(self, tmp_path):
        """Malformed hex string must not raise ValueError — returns mismatch instead."""
        path = self._write_file(tmp_path, b"data")
        status = check_file_status(path, "0xINVALID", None)
        assert status == "mismatch"

    def test_invalid_hex_patched_crc_returns_mismatch_not_crash(self, tmp_path):
        path = self._write_file(tmp_path, b"data")
        status = check_file_status(path, None, "NOT_HEX!")
        assert status == "mismatch"

    def test_crc_with_0x_prefix_returns_mismatch_not_crash(self, tmp_path):
        """patch.json authors sometimes write '0x' prefixed values — must not crash."""
        path = self._write_file(tmp_path, b"data")
        status = check_file_status(path, "0xDEADBEEF", None)
        assert status == "mismatch"
