//! 随图保存的协同状态（`REQ-COLL-015`）：两条图级 XRecord，挂在模型空间
//! block record 的扩展字典下，与 [`super::graph_data`] 的四个 key 同层。
//!
//! * [`PID_AUTHOR_COLLAB`]：一条记录，三项——图号 `drawing_id`、服务器库
//!   标识 `repo_id`、已应用到的修订 `revision`。它们说明的是「这份文件
//!   上一次对齐的是哪个库、对齐到哪儿」；下次加入时客户端按它们对账。
//! * [`PID_AUTHOR_COLLAB_QUEUE`]：已做出但房间尚未回答的本地修改，一条
//!   op 一条记录，顺序即队列顺序。进程结束后它们不再在内存里，靠这条
//!   记录回到队列重新提交。
//!
//! 两条记录只在保存快照里写（`document_for_save` 之后），不动活文档：
//! 不弄脏 `dirty`，也不进撤销栈。不连服务器的图没有这两条，也不受影响
//! ——打开、绘制、保存都照常。

use document::Document;

use super::graph_data;

/// 三项协同状态的 XRecord key。
pub const PID_AUTHOR_COLLAB: &str = "PID_AUTHOR_COLLAB";

/// 未回答 op 队列的 XRecord key。
pub const PID_AUTHOR_COLLAB_QUEUE: &str = "PID_AUTHOR_COLLAB_QUEUE";

/// 这份文件上一次对齐到的协同位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCollabState {
    /// 服务器登记的图号。
    pub drawing_id: String,
    /// 服务器库的标识（`meta.repo_id`）。
    pub repo_id: String,
    /// 已应用到的修订。
    pub revision: i64,
}

/// 一条未回答的 op，按它出队重发时需要的样子。
///
/// 不直接序列化 `collab::Outgoing`：撤销栈步号（`steps`）活不过进程，
/// 持久化它只会假装还能撤回；`restored`/`sent` 是运行时状态。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct QueuedOp {
    pub op_id: String,
    pub payload: serde_json::Value,
    /// 这步做出来时本图应用到的修订：op 按它被审判。
    pub base_revision: i64,
    /// 步是放上（made / redone）还是拿下（undone）。
    pub on: bool,
    /// 它叫什么，被拒时的那句话用。
    pub label: String,
    /// 谁排的：排队时的用户号。只有排队的人自己提交它。旧版本存的条目没有
    /// 这一项。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// 什么时候排的，Unix 秒。旧版本存的条目没有这一项。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_at: Option<i64>,
}

/// 读回的队列：读得出的条目，和读不出的那些。
#[derive(Clone, Debug, Default)]
pub struct QueueRead {
    /// 按写入顺序。
    pub ops: Vec<QueuedOp>,
    /// 读不出的条目，原样：是 JSON 但不是一条 op 的，就是那个 JSON；连
    /// JSON 都不是的，是它的原文（一个 JSON 字符串）。它们也是没提交的修改，
    /// 存盘时原样写回去（[`store_queue_keeping`]），不能就这样丢掉。
    pub unreadable: Vec<serde_json::Value>,
}

/// 写入（或清空）三项协同状态。`None` 表示这份文件不从属于任何库——
/// 记录被删掉，不留残值。
pub fn store_state(doc: &mut Document, state: Option<&FileCollabState>) {
    let items = match state {
        Some(state) => vec![serde_json::json!({
            "drawing_id": state.drawing_id,
            "repo_id": state.repo_id,
            "revision": state.revision,
        })],
        None => Vec::new(),
    };
    graph_data::store(doc, PID_AUTHOR_COLLAB, &items);
}

/// 读回三项协同状态。记录不存在、残缺或字段非法都返回 `None`：一份
/// 对不上的记录比没有记录更危险，宁可按「从未对齐过」处理。
pub fn load_state(doc: &Document) -> Option<FileCollabState> {
    let items = graph_data::load(doc, PID_AUTHOR_COLLAB)?;
    let item = items.first()?;
    Some(FileCollabState {
        drawing_id: item["drawing_id"].as_str()?.to_owned(),
        repo_id: item["repo_id"].as_str()?.to_owned(),
        revision: item["revision"].as_i64()?,
    })
}

/// 整队列重写：先清后填。空队列删除记录，不留空 XRecord。
pub fn store_queue(doc: &mut Document, ops: &[QueuedOp]) {
    store_queue_keeping(doc, ops, &[]);
}

/// 整队列重写，读不出的条目 `unreadable` 原样排在后面（[`QueueRead`]）。
pub fn store_queue_keeping(doc: &mut Document, ops: &[QueuedOp], unreadable: &[serde_json::Value]) {
    let items: Vec<serde_json::Value> = ops
        .iter()
        .filter_map(|op| serde_json::to_value(op).ok())
        .chain(unreadable.iter().cloned())
        .collect();
    graph_data::store(doc, PID_AUTHOR_COLLAB_QUEUE, &items);
}

/// 读回队列：按写入顺序。解析失败的一条被跳过，不拖垮整队；要知道跳过
/// 了哪些，用 [`read_queue`]。
pub fn load_queue(doc: &Document) -> Vec<QueuedOp> {
    read_queue(doc).ops
}

/// 读回队列，连同读不出的条目。
pub fn read_queue(doc: &Document) -> QueueRead {
    let Some(loaded) = graph_data::load_reporting(doc, PID_AUTHOR_COLLAB_QUEUE) else {
        return QueueRead::default();
    };
    let mut read = QueueRead::default();
    for item in loaded.items {
        match serde_json::from_value::<QueuedOp>(item.clone()) {
            Ok(op) => read.ops.push(op),
            Err(_) => read.unreadable.push(item),
        }
    }
    read.unreadable
        .extend(loaded.unreadable.into_iter().map(serde_json::Value::String));
    read
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        Document::new()
    }

    #[test]
    fn collab_state_round_trips_and_clears() {
        let mut doc = doc();
        assert_eq!(load_state(&doc), None, "无记录即未对齐过");
        let state = FileCollabState {
            drawing_id: "PID-1101".into(),
            repo_id: "r-abc".into(),
            revision: 42,
        };
        store_state(&mut doc, Some(&state));
        assert_eq!(load_state(&doc), Some(state));
        store_state(&mut doc, None);
        assert_eq!(load_state(&doc), None, "清空后记录不存在");
        assert_eq!(graph_data::item_count(&doc, PID_AUTHOR_COLLAB), 0);
    }

    #[test]
    fn collab_queue_round_trips_in_order_and_clears() {
        let mut doc = doc();
        assert!(load_queue(&doc).is_empty());
        let ops: Vec<QueuedOp> = (0..3)
            .map(|i| QueuedOp {
                op_id: format!("op-{i}"),
                payload: serde_json::json!({"kind": "set_property", "i": i}),
                base_revision: i as i64,
                on: true,
                label: format!("步骤 {i}"),
                user_id: None,
                queued_at: None,
            })
            .collect();
        store_queue(&mut doc, &ops);
        let back = load_queue(&doc);
        assert_eq!(
            back.iter().map(|op| op.op_id.as_str()).collect::<Vec<_>>(),
            ["op-0", "op-1", "op-2"]
        );
        store_queue(&mut doc, &[]);
        assert!(load_queue(&doc).is_empty());
        assert_eq!(graph_data::item_count(&doc, PID_AUTHOR_COLLAB_QUEUE), 0);
    }
}
