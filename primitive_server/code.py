from pathlib import Path

ROOT = Path(".")

# Расширения, которые считаем текстовым кодом.
# Добавь свои при необходимости.
EXTENSIONS = {
    ".py", ".rs", ".c", ".h", ".cpp", ".hpp",
    ".dpt", ".dasm", ".dyc", ".js", ".ts",
    ".html", ".css", ".java", ".cs", ".go",
    ".lua", ".php", ".rb", ".pl", ".sh"
}


def count_lines(path: Path) -> int:
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as f:
            return sum(1 for _ in f)
    except Exception:
        return 0


def scan_folder(folder: Path):
    files_total = 0
    lines_total = 0

    print(f"\n📁 {folder}")

    for path in sorted(folder.iterdir()):
        if path.is_file() and path.suffix.lower() in EXTENSIONS:
            lines = count_lines(path)
            files_total += 1
            lines_total += lines
            print(f"   {path.name:<40} {lines:>8} строк")

    print(f"   {'ИТОГО':<40} {lines_total:>8} строк ({files_total} файлов)")
    return files_total, lines_total


def main():
    total_files = 0
    total_lines = 0

    # Сначала корневая папка
    f, l = scan_folder(ROOT)
    total_files += f
    total_lines += l

    # Затем все вложенные папки
    for folder in sorted(p for p in ROOT.rglob("*") if p.is_dir()):
        f, l = scan_folder(folder)
        total_files += f
        total_lines += l

    print("\n" + "=" * 60)
    print(f"ВСЕГО ФАЙЛОВ: {total_files}")
    print(f"ВСЕГО СТРОК:  {total_lines}")
    print("=" * 60)


if __name__ == "__main__":
    main()