//! 位号生成性能预算（`VAL-SEM-014`）。
//!
//! 2000 实例批量生成 ≤ 3000 ms，`#[ignore]` 标记，门禁时用
//! `cargo test -- --ignored` 单独跑。

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::time::{Duration, Instant};

    use codec::entities::Line;
    use codec::types::Vector3;
    use codec::Document;

    use crate::pid::persist::PidAuthor;
    use crate::pid::tag_engine::TagSequencer;
    use crate::pid::tag_rule::{HierarchyMode, TagRule};
    use crate::pid::type_lib::{TypeLib, TypeNode};

    /// 契约规模：空文档中批量插入 2000 个 `EQUIPMENT` 实例。
    const INSTANCES: usize = 2000;
    /// 契约预算：第一个实例创建 → 第 2000 个位号落定 ≤ 3000 ms。
    const BUDGET_MS: u128 = 3000;
    /// 采样次数：取中位数，减少单次抖动误判。
    const RUNS: usize = 3;

    fn line() -> codec::EntityType {
        codec::EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        ))
    }

    /// 契约规则：`P-{序号}` / 起始 1001 / 增量 1 / `当前类型依次递增` /
    /// 唯一校验开启。挂在 `EQUIPMENT` 上。
    fn lib() -> TypeLib {
        let mut rule = TagRule::new("P-{序号}", 1001);
        rule.step = 1;
        rule.level = Some(HierarchyMode::CurrentTypeIncrement);
        rule.unique = true;
        TypeLib::new(vec![
            TypeNode::new("EQUIPMENT", None::<String>)
                .with_label("设备")
                .with_tag_rule(rule),
        ])
        .expect("perf fixture type lib must build")
    }

    /// 一轮完整批量插入：空文档连插 2000 个 `EQUIPMENT`，返回耗时与位号序列。
    ///
    /// 计时区间按契约取「第一个实例创建 → 第 2000 个位号落定」：
    /// `add_entity` → `next` → `PidAuthor::store` 全部在环内。
    ///
    /// 批量路径用 [`TagSequencer`]：`new` 扫一次空文档，之后每次 `next`
    /// 只查表推进，避免 `generate` 每实例重扫全文档的 O(n²)。
    fn run_once(lib: &TypeLib) -> (Duration, Vec<String>) {
        let mut doc = Document::new();
        let mut tags = Vec::with_capacity(INSTANCES);
        let started = Instant::now();
        let mut seq = TagSequencer::new(lib, &doc);
        for _ in 0..INSTANCES {
            let handle = doc.add_entity(line()).expect("entity added");
            let view = seq
                .next("EQUIPMENT", &BTreeMap::new())
                .expect("tag generation must succeed");
            PidAuthor {
                type_code: "EQUIPMENT".to_string(),
                tag: view.tag.clone(),
                tag_source: view.tag_source.clone(),
                tag_status: view.tag_status.clone(),
                props: "{}".to_string(),
                ..PidAuthor::default()
            }
            .store(&mut doc, handle);
            tags.push(view.tag);
        }
        (started.elapsed(), tags)
    }

    #[test]
    #[ignore]
    fn tag_rule_bulk_2000_under_three_seconds() {
        let lib = lib();

        let mut samples = Vec::with_capacity(RUNS);
        let mut last_tag = String::new();
        let mut unique = 0usize;
        for run in 1..=RUNS {
            let (elapsed, tags) = run_once(&lib);
            last_tag = tags.last().cloned().unwrap_or_default();
            unique = tags.iter().collect::<HashSet<_>>().len();
            println!(
                "VAL-SEM-014 run {run}/{RUNS}: {} ms, 末个位号 {last_tag}, 去重后 {unique}",
                elapsed.as_millis()
            );
            assert_eq!(
                (tags.last().map(String::as_str), unique),
                (Some("P-3000"), INSTANCES),
                "run {run}: 实测 {} ms，末个位号 {last_tag}，去重后 {unique} / {}",
                elapsed.as_millis(),
                tags.len()
            );
            samples.push(elapsed);
        }

        samples.sort();
        let median = samples[RUNS / 2];
        println!(
            "VAL-SEM-014 实测（中位数 {} ms）: {:?}，预算 {BUDGET_MS} ms",
            median.as_millis(),
            samples.iter().map(Duration::as_millis).collect::<Vec<_>>()
        );
        assert!(
            median.as_millis() <= BUDGET_MS,
            "VAL-SEM-014 超预算：实测中位数 {} ms（{:?}），末个位号 {last_tag}，去重后 {unique}",
            median.as_millis(),
            samples.iter().map(Duration::as_millis).collect::<Vec<_>>()
        );
    }
}
