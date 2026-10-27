"""Markdown 链接与锚点契约：解析 docs/pid-platform 下的相对链接并校验其有效性。

本模块只做三件事：抽取行内链接、按源文件所在目录解析相对路径、
按 GitHub 规则归一化标题锚点。所有函数都作用在真实文件系统上。
"""
from __future__ import annotations

import re
from pathlib import Path

# ATX 标题：可选的收尾井号不算标题文本。
HEADING_RE = re.compile(r"^(#{1,6})[ \t]+(.+?)[ \t]*#*[ \t]*$", re.MULTILINE)

# 行内链接 [text](target)；只取目标，不解析引用式链接。
INLINE_LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")

# 围栏代码块内的方括号不是链接，先整体剥离。
CODE_FENCE_RE = re.compile(r"^```.*?^```", re.MULTILINE | re.DOTALL)

EXTERNAL_PREFIXES = ("http://", "https://", "mailto:", "tel:", "//", "data:")

# 保留字母、数字、下划线、连字符、汉字与空格；其余标点一律丢弃。
_NON_SLUG_RE = re.compile(r"[^\w\u4e00-\u9fff\- ]+", re.UNICODE)


def strip_code_fences(text: str) -> str:
    """去掉围栏代码块，避免把示例语法当成真实链接。"""
    return CODE_FENCE_RE.sub("", text)


def link_targets(text: str) -> list:
    """按出现顺序返回文档中的全部行内链接目标。"""
    return [m.group(1).strip() for m in INLINE_LINK_RE.finditer(strip_code_fences(text))]


def split_target(target: str):
    """把链接目标拆成 (路径, 锚点)；无锚点时锚点为 None。"""
    if "#" in target:
        path, _, anchor = target.partition("#")
        return path.strip(), (anchor.strip() or None)
    return target.strip(), None


def is_external(target: str) -> bool:
    """外链不参与本地文件校验。"""
    return target.lower().startswith(EXTERNAL_PREFIXES)


def github_anchor(heading_text: str) -> str:
    """把标题文本转成 GitHub 风格的锚点串。

    规则：去反引号与强调符、转小写、去标点、空格转连字符。
    """
    text = heading_text.strip()
    text = text.replace("`", "").replace("*", "")
    text = text.lower()
    text = _NON_SLUG_RE.sub("", text)
    return text.strip().replace(" ", "-")


def anchors_of(path) -> set:
    """返回某个 Markdown 文件的全部标题锚点。"""
    path = Path(path)
    if not path.exists():
        return set()
    text = strip_code_fences(path.read_text(encoding="utf-8"))
    return {github_anchor(m.group(2)) for m in HEADING_RE.finditer(text)}


def check_links(root):
    """校验 root 下全部 Markdown 文件的相对链接与锚点。

    返回 (errors, checked)：errors 为错误消息列表，checked 为实际检查的
    相对链接条数。外链跳过且不计入 checked。
    """
    root = Path(root)
    errors = []
    checked = 0
    for source in sorted(root.rglob("*.md")):
        source_rel = source.relative_to(root).as_posix()
        for raw in link_targets(source.read_text(encoding="utf-8")):
            if is_external(raw):
                continue
            checked += 1
            target_path, anchor = split_target(raw)
            resolved = source if target_path == "" else (source.parent / target_path)
            resolved = resolved.resolve()
            if not resolved.exists():
                errors.append("{0}: 断链 {1} -> {2} 不存在".format(source_rel, raw, target_path))
                continue
            if anchor is not None and resolved.suffix.lower() == ".md":
                if anchor not in anchors_of(resolved):
                    errors.append("{0}: 锚点缺失 {1} -> #{2}".format(source_rel, raw, anchor))
    return errors, checked
