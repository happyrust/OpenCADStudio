"""Baseline harness checks: deliverable tree exists and helpers behave."""
from conftest import DOCS, PROTO, normalize_heading


def test_deliverable_tree_exists():
    assert DOCS.is_dir(), "missing deliverable dir: " + str(DOCS)
    assert PROTO.is_dir(), "missing prototype dir: " + str(PROTO)


def test_heading_normalization():
    assert normalize_heading("## 1. 产品定位") == "产品定位"
    assert normalize_heading("## 2.3 目标用户与角色") == "目标用户与角色"
    assert normalize_heading("## 路线图") == "路线图"
