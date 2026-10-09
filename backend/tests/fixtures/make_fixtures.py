#!/usr/bin/env python3
"""產生清冊稽核整合測試用的合成 Excel fixtures（僅 Python 標準庫）。

用法（於 repo 任意位置執行）：
    python3 backend/tests/fixtures/make_fixtures.py

產出（與本檔同目錄；全部為合成資料，不含真實學校資料）：
    property.xlsx / property.xls  財產清冊（多行編號、略過列、頁尾、空白三聯頁）
    items.xlsx                    物品清冊（種類＝物品）
    header_only.xlsx              僅有表頭、無資料列
    no_header.xlsx                無「財產編號」表頭
    many_rows.xlsx                5,001 資料列（列數上限拒絕用）
    max_rows.xlsx                 5,000 資料列（列數上限邊界允許用）

.xlsx 以 zipfile＋inlineStr 手工組出（環境無 openpyxl）；.xls 以
`libreoffice --headless --convert-to xls` 轉出。
"""

import os
import subprocess
import sys
import zipfile
from xml.sax.saxutils import escape

HERE = os.path.dirname(os.path.abspath(__file__))


def col_ref(index):
    """0-based 欄索引 → Excel 欄名（A、B、…、AA）。"""
    name = ""
    index += 1
    while index:
        index, remainder = divmod(index - 1, 26)
        name = chr(ord("A") + remainder) + name
    return name


def cell_xml(row, col, value):
    if value is None or value == "":
        return ""
    ref = f"{col_ref(col)}{row}"
    text = escape(str(value))
    return f'<c r="{ref}" t="inlineStr"><is><t xml:space="preserve">{text}</t></is></c>'


def sheet_xml(rows):
    parts = [
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>',
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>',
    ]
    for row_index, row in enumerate(rows, start=1):
        cells = "".join(cell_xml(row_index, col, value) for col, value in enumerate(row))
        if cells:
            parts.append(f'<row r="{row_index}">{cells}</row>')
    parts.append("</sheetData></worksheet>")
    return "".join(parts)


def content_types_xml(sheet_count):
    overrides = "".join(
        f'<Override PartName="/xl/worksheets/sheet{index}.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        for index in range(1, sheet_count + 1)
    )
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
        '<Override PartName="/xl/styles.xml" '
        'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'
        f"{overrides}</Types>"
    )


def workbook_xml(sheet_names):
    sheets = "".join(
        f'<sheet name="{escape(name)}" sheetId="{index}" r:id="rId{index}"/>'
        for index, name in enumerate(sheet_names, start=1)
    )
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
        'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
        f"<sheets>{sheets}</sheets></workbook>"
    )


def workbook_rels_xml(sheet_count):
    rels = "".join(
        f'<Relationship Id="rId{index}" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" '
        f'Target="worksheets/sheet{index}.xml"/>'
        for index in range(1, sheet_count + 1)
    )
    styles_id = sheet_count + 1
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        f"{rels}"
        f'<Relationship Id="rId{styles_id}" '
        'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" '
        'Target="styles.xml"/></Relationships>'
    )


ROOT_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" '
    'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
    'Target="xl/workbook.xml"/></Relationships>'
)

STYLES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
    '<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>'
    '<fills count="2"><fill><patternFill patternType="none"/></fill>'
    '<fill><patternFill patternType="gray125"/></fill></fills>'
    '<borders count="1"><border/></borders>'
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
    '<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>'
    '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>'
    "</styleSheet>"
)


def write_xlsx(path, sheets):
    """sheets＝[(工作表名稱, 列清單)]；列清單元素為 0-based 欄值的 list。"""
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("[Content_Types].xml", content_types_xml(len(sheets)))
        archive.writestr("_rels/.rels", ROOT_RELS)
        archive.writestr("xl/workbook.xml", workbook_xml([name for name, _ in sheets]))
        archive.writestr("xl/_rels/workbook.xml.rels", workbook_rels_xml(len(sheets)))
        archive.writestr("xl/styles.xml", STYLES)
        for index, (_, rows) in enumerate(sheets, start=1):
            archive.writestr(f"xl/worksheets/sheet{index}.xml", sheet_xml(rows))


PROPERTY_SHEET = [
    ["資訊組財產盤點清冊"],
    ["造冊日期：2026-10-09"],
    ["財產編號", "財產名稱", "形式\n品名", "存放地點", "保管人員", "備註", "未知欄位"],
    ["3140101-03\n\n3000380", "伺服器主機", "機架式\n2U", "電腦機房", "王小明", None, "未知值"],
    ["PC-100", "個人電腦", "桌上型", "辦公室A", "李四", "備註甲", None],
    [" pc-100\n", "個人電腦（重複）", "桌上型", "辦公室B", "李四", None, None],
    ["無帳", "待報廢設備", "雜項", "倉庫", "王小明", None, None],
    ["保管人：王小明"],
    ["說明：本表僅供盤點核對"],
    ["第 一 聯"],
    ["本單遺失請洽財管單位"],
]

BLANK_TRIPLICATE_SHEET = [
    [],
    [],
    ["財產編號", "財產名稱", "形式品名", "存放地點", "保管人員", "備註"],
]

ITEMS_SHEET = [
    ["資訊組物品盤點清冊"],
    ["財產編號", "財產名稱", "形式品名", "存放地點", "保管人員", "備註"],
    ["60102\n6005129", "雷射印表機", "HP M404", "辦公室B", "陳五", None],
    ["\u3000x3009999\u3000", "x 前綴編號", "測試機種", "辦公室C", "陳五", None],
]

HEADER_ONLY_SHEET = [
    ["僅表頭清冊"],
    ["財產編號", "財產名稱", "備註"],
    ["保管人：王小明"],
]

NO_HEADER_SHEET = [
    ["無表頭清冊"],
    ["編號", "名稱"],
    ["3001", "電腦"],
]


def main():
    write_xlsx(
        os.path.join(HERE, "property.xlsx"),
        [("財產清冊", PROPERTY_SHEET), ("空白三聯", BLANK_TRIPLICATE_SHEET)],
    )
    write_xlsx(os.path.join(HERE, "items.xlsx"), [("物品清冊", ITEMS_SHEET)])
    write_xlsx(os.path.join(HERE, "header_only.xlsx"), [("僅表頭", HEADER_ONLY_SHEET)])
    write_xlsx(os.path.join(HERE, "no_header.xlsx"), [("無表頭", NO_HEADER_SHEET)])
    write_xlsx(
        os.path.join(HERE, "many_rows.xlsx"),
        [
            (
                "超限",
                [["財產編號", "財產名稱"]]
                + [[f"P{index:05d}", f"設備{index}"] for index in range(1, 5002)],
            )
        ],
    )
    write_xlsx(
        os.path.join(HERE, "max_rows.xlsx"),
        [
            (
                "上限",
                [["財產編號", "財產名稱"]]
                + [[f"P{index:05d}", f"設備{index}"] for index in range(1, 5001)],
            )
        ],
    )

    # .xls（BIFF）：以 LibreOffice 轉出 property.xlsx。
    result = subprocess.run(
        [
            "libreoffice",
            "--headless",
            "--convert-to",
            "xls",
            "--outdir",
            HERE,
            os.path.join(HERE, "property.xlsx"),
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0 or not os.path.exists(os.path.join(HERE, "property.xls")):
        print(result.stdout, file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        raise SystemExit("LibreOffice 轉出 property.xls 失敗")

    for name in [
        "property.xlsx",
        "property.xls",
        "items.xlsx",
        "header_only.xlsx",
        "no_header.xlsx",
        "many_rows.xlsx",
        "max_rows.xlsx",
    ]:
        path = os.path.join(HERE, name)
        print(f"{name}: {os.path.getsize(path)} bytes")


if __name__ == "__main__":
    main()
