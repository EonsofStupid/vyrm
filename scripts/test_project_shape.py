import tempfile
import unittest
from pathlib import Path

from project_shape import build_shape, render_markdown


class ProjectShapeTests(unittest.TestCase):
    def test_shape_is_sorted_content_addressed_and_excludes_build_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "example"
            (root / "src").mkdir(parents=True)
            (root / "target").mkdir()
            (root / "src" / "z.rs").write_text("fn z() {}\n")
            (root / "README.md").write_text("# Example\n")
            (root / "target" / "ignored").write_text("noise")

            first = build_shape(root)
            second = build_shape(root)

            self.assertEqual(first, second)
            self.assertEqual(
                [entry["path"] for entry in first["files"]],
                ["README.md", "src/z.rs"],
            )
            self.assertIn("`README.md`", render_markdown(first))

    def test_tree_identity_changes_with_file_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "main.py"
            source.write_text("print(1)\n")
            before = build_shape(root)["tree_sha256"]
            source.write_text("print(2)\n")
            self.assertNotEqual(before, build_shape(root)["tree_sha256"])


if __name__ == "__main__":
    unittest.main()
