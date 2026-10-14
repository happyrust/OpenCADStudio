# `e3d-model` 接入 `OpenCADStudio` 开发计划

> 关联文档：
> - 领域术语表：[`GLOSSARY.md`](../GLOSSARY.md)
> - 架构决策 0001（双端同构契约与服务端生成）：[`docs/adr/0001-e3d-scene-batch-dual-target-architecture.md`](adr/0001-e3d-scene-batch-dual-target-architecture.md)
> - 架构决策 0002（轻量 CAD 代理实体、实例共享与 BevyBridge M2 镜像）：[`docs/adr/0002-e3d-geometry-proxy-and-bevy-mirror.md`](adr/0002-e3d-geometry-proxy-and-bevy-mirror.md)

---

## 1. 总体架构概览

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                        原生生产侧 (cfg(not(wasm32)))                         │
│                                                                              │
│  ┌─────────────────────────┐  ┌──────────────────────┐  ┌─────────────────┐  │
│  │ Dabacon 工程库 (e3d-io) │  │ 增量快照比对         │  │ .rvm 文件解析   │  │
│  │ DirectoryDbResolver     │  │ e3d_model::increment │  │ e3d_model::     │  │
│  │ generate_subtree_with_  │  │ ::increment_update   │  │ rvm_read        │  │
│  │ cache + CatalogueCache  │  │ (L1→L2→L3→L4)        │  │                 │  │
│  └────────────┬────────────┘  └──────────┬───────────┘  └────────┬────────┘  │
│               └──────────────────────────┼───────────────────────┘           │
│                                          ▼                                   │
│                 ┌─────────────────────────────────────────┐                  │
│                 │   E3dSceneBatch 组装器 (producer.rs)    │                  │
│                 │ • E3dBatchManifest (实例/中心线/总账)   │                  │
│                 │ • Content-Addressed MeshPool (MeshKey)  │                  │
│                 └───────────┬─────────────────────┬───────┘                  │
│                             │ 进程内通道          │ HTTP (/api/v1/e3d/*)     │
└─────────────────────────────┼─────────────────────┼──────────────────────────┘
                              │ (Native Desktop)    │ (Web WASM / 远程客户端)
                              ▼                     ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                     双端统一消费侧 (Native + wasm32)                         │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │                 src/io/e3d/consumer.rs (E3dBatchIngestor)              │  │
│  │  • GeometryIdKey <-> Handle 索引表（支持全量装载 + O(Δ) 会话增量补丁） │  │
│  │  • 本地 MeshKey 缓存池（Web 端仅对缺失 MeshKey 调 /api/v1/e3d/meshes） │  │
│  └──────────────────┬───────────────────────────────────┬─────────────────┘  │
│                     ▼                                   ▼                    │
│  ┌─────────────────────────────────────┐ ┌─────────────────────────────────┐ │
│  │ CadDocument + Scene::meshes         │ │ BevyBridge M2 (feature=bevy3d)  │ │
│  │ • 轻量代理实体 + E3D_SEMANTICS XDATA│ │ • MeshKey -> Shared Handle<Mesh>│ │
│  │ • MeshInstanceSource 共享盒/圆柱/件 │ │ • 100m 空间分块原点重设 (f64→f32│ │
│  │ • 双单精度 (verts + verts_low)      │ │   消除大坐标抖动)               │ │
│  │ • 中心线 / 无效管身虚线 (WireModel) │ │ • Reversed-Z 深度与原生线框合成 │ │
│  └─────────────────────────────────────┘ └─────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 模块划分与目录结构

在 `OpenCADStudio` 中新增 `src/io/e3d/` 模块体系，并与 `src/rest.rs`、`src/scene/`、`src/scene/pipeline/bevy_bridge.rs` 协同：

- `src/io/e3d/mod.rs`：模块入口，向 `src/io/mod.rs` 暴露统一加载、增量刷新与只读源判定接口。
- `src/io/e3d/contract.rs`（**Native + WASM 共用，纯 Rust**）：
  - `GeometryIdKey`：对应 `Element { refno }` 与 `ImpliedTube { container_refno, route_ordinal, from_refno, to_refno }` 的稳定序列化键；
  - `MeshKey`：区分 `Canonical(PrimitiveMeshKey)`（`BoxV1`, `CylinderV3 { segments }`）、`Catalogue(RefNo, u64)` 与 `BakedSha256([u8; 32])`；
  - `E3dInstanceRecord`：`geometry_id`, `refno`, `owner_refno`, `noun`, `name`, `world_matrix: [f64; 16]`, `local_aabb_mm`, `mesh_key`, `color_hint`；
  - `E3dCentrelineRecord`：区分 `ProfileCentreline`（`SCTN` 无 `SPRE`）、`NodeMarker`（`SNOD`）与 `InvalidTubi`（`is_invalid_tubi == true`，虚线）；
  - `E3dLedgerSummary`：映射 `e3d_model::Report`（严格携带 `tree_total`, `visited`, `consumed` 恒等校验）；
  - `E3dBatchManifest`：`is_incremental`, `upserted: Vec<E3dInstanceRecord>`, `centrelines: Vec<E3dCentrelineRecord>`, `removed: Vec<GeometryIdKey>`, `required_meshes: Vec<MeshKey>`, `ledger: E3dLedgerSummary`；
  - `CompactTriMesh`：紧凑索引网格（`positions_f32`, `normals_f32`, `indices_u32`, `feature_edges_u32`）。
- `src/io/e3d/producer.rs`（**仅 `#[cfg(not(target_arch = "wasm32"))]`**）：
  - 封装 `e3d_io::db_discovery::DirectoryDbResolver`；
  - 封装 `e3d_model::generate_subtree_with_cache` 与 `CatalogueMeshCache`；
  - 封装 `e3d_model::increment::increment_update`（维护会话快照 `SessionWindow`）；
  - 封装 `e3d_model::rvm_read::RvmFile` 解析器；
  - 将 `Manifold::mesh()` 的 `TriMesh`（`f64` mm）转为局部原点归心后的 `CompactTriMesh` + 提取特征折边（Crease Edges），并写入内存/磁盘内容寻址缓存。
- `src/io/e3d/server_api.rs`（**仅 `#[cfg(not(target_arch = "wasm32"))]`，挂载于 `src/rest.rs`**）：
  - 在 `OpenCADStudio --http <port>` 中新增 `/api/v1/e3d/*` 端点：
    - `GET  /api/v1/e3d/projects`：列出已挂载的 E3D 工程库 / `.rvm` 文件与 `dbnum` 状态；
    - `POST /api/v1/e3d/tree`：按 `refno` 查询子树层级（`SITE` -> `ZONE` -> `PIPE`/`STRU`/`EQUI`）与构件属性（`e3d-attlib`）；
    - `POST /api/v1/e3d/generate`：按根 `refno` 列表（或 `.rvm` 路径）生成并返回 `E3dBatchManifest`；
    - `POST /api/v1/e3d/increment`：对指定 `dbnum` 执行会话对比（`from_session -> to_session`），返回增量 `E3dBatchManifest`（仅含 `Regenerate` 与 `Remove`）；
    - `POST /api/v1/e3d/meshes`：客户端传入缺失的 `Vec<MeshKey>`，批量返回紧凑二进制网格包；
  - 同时预留 `gen_model_adapter.rs`：可选将外部 `gen-model` 服务的 `/model/records` + `meshes://<hash>.mesh` 适配为 `E3dBatchManifest`，满足企业级集中供数场景。
- `src/io/e3d/web_client.rs`（**Native 远程模式 + `wasm32` 共用**）：
  - 复用 `src/io/web_link.rs` 的异步请求机制，请求 `/api/v1/e3d/generate` 或 `/increment`，对比本地已缓存的 `MeshKey` 集合，仅拉取差集网格后合成完整批次交给 `consumer.rs`。
- `src/io/e3d/consumer.rs`（**Native + WASM 共用**）：
  - 将 `E3dBatchManifest` + `CompactTriMesh` 注入 `CadDocument` 与 `Scene`：
    - 按专业/构件类型自动建层（`E3D_PIPE`, `E3D_EQUI`, `E3D_STRU`, `E3D_HVAC`, `E3D_CENTRELINE`, `E3D_INVALID_TUBI`）；
    - 为每个 `GeometryIdKey` 创建或更新轻量代理实体，挂载 `E3D_SEMANTICS` XDATA；
    - 共享 `MeshKey` 注册到 `Scene` 共享块网格池，实例通过 `MeshInstanceSource` 携带 `DMat4` 变换；
    - 维护 `Scene` 内的 `e3d_index: HashMap<GeometryIdKey, Handle>`，增量刷新时按 `removed` 精准卸载旧句柄、按 `upserted` 原地替换。

---

## 3. 分阶段里程碑（Milestones）与交付物

### Phase 0：工作区环境与依赖挂载（半天）
1. **目录联接**：在 `D:\work\plant-code\cad\` 下建立指向 `D:\work\plant-code\old\vendor\{e3d-model, e3d-io, e3d-attlib}` 的目录联接（Junction），使相对路径 `../e3d-model`、`../e3d-io`、`../e3d-attlib` 生效。
2. **工作树准备**：收口主目录 merge 冲突或切入干净 worktree（如 `ocs-main-with-upstream`）。
3. **条件编译配置**：在 `OpenCADStudio/Cargo.toml` 的 `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` 下引入 `e3d-model`、`e3d-io`、`e3d-attlib`（可选挂于 `e3d` feature 下，默认开启）。
4. **验收标准**：
   - `cargo check`（桌面原生）与 `cargo check --target wasm32-unknown-unknown`（Web 端）双端零错误通过。

### Phase 1：同构传输契约 `E3dSceneBatch` 与原生生成组装器（2 天）
1. 实现 `src/io/e3d/contract.rs`（`GeometryIdKey`, `MeshKey`, `E3dBatchManifest`, `CompactTriMesh`）。
2. 实现 `src/io/e3d/producer.rs`：
   - 接入 `generate_subtree_with_cache`，将 `PrimitiveInstance`（`BoxV1` / `CylinderV3`）拆分为规范网格 + `world * local_transform` 实例矩阵；
   - 将布尔后的 `Manifold::mesh()` 转换为局部归心 `CompactTriMesh`（消除个体网格内部大坐标偏移）并计算 SHA-256 `MeshKey`；
   - 收集 `is_invalid_tubi` 与 `Report::centreline_only` 生成 `E3dCentrelineRecord`；
   - 接入 `rvm_read::RvmFile`，支持把 `.rvm` 文件同样转换为 `E3dBatchManifest`。
3. **验收标准**：
   - 单元测试使用 `e3d-model` 的样本库（如 `PIPE 26229/1509` 或测试 `.rvm`）跑通 `producer -> E3dBatchManifest`，断言 `visited + consumed == tree_total` 恒成立，且规范盒/圆柱在 `MeshPool` 中只生成 1 份网格。

### Phase 2：`CadDocument` / `Scene` 消费器与 In-house 渲染管线打通（2 天）
1. 实现 `src/io/e3d/consumer.rs`：
   - 在 `CadDocument` 中注册轻量只读代理实体与 `E3D_SEMANTICS` XDATA，在 `is_read_only_source_path` 中保护 `.rvm` 与 Dabacon 目录不被直接覆写；
   - 将 `MeshKey` 生成 `MeshLodSet`（含 3 级 LOD 与特征边 `edge_verts`），通过 `MeshInstanceSource` 注入 `Scene::meshes`；
   - 将 `InvalidTubi`（虚线）与 `ProfileCentreline` / `NodeMarker`（实线）注入 `WireModel`。
2. 接入文件打开入口与属性面板：
   - 支持打开 `.rvm` 文件及选择 E3D 工程目录；
   - 点击三维构件时，属性面板展示 `RefNo`、`Noun`、`Name`、所属 `SITE/ZONE/BRAN` 及 `e3d-attlib` 工程属性。
3. **验收标准**：
   - 桌面端默认渲染管线可打开 E3D 子树与 `.rvm` 文件，构件拾取、属性查看、按 `Noun` 图层开关、剖切平面（Section Clip）全部正常工作。

### Phase 3：服务端 HTTP 接口与 Web（WASM）客户端闭环（2 天）
1. 在 `src/rest.rs` 中挂载 `/api/v1/e3d/{projects,tree,generate,increment,meshes}` 端点（并启用 CORS 以供浏览器跨域访问）。
2. 实现 `src/io/e3d/web_client.rs`：
   - Web 端通过 `fetch` 请求 `/api/v1/e3d/generate` 拿到 `E3dBatchManifest`；
   - 过滤本地尚未缓存的 `MeshKey`，调用 `/api/v1/e3d/meshes` 批量拉取紧凑二进制网格；
   - 交由同一个 `consumer.rs` 注入 Web 端 `Scene`。
3. （可选扩展）提供 `gen_model_adapter.rs`，支持将 `gen-model` 的 `/model/records` + `.mesh` 直接映射为 `E3dBatchManifest`。
4. **验收标准**：
   - 启动 `OpenCADStudio --http 8080`，在浏览器运行 WASM 版 `OpenCADStudio`，通过服务端生成并流畅加载同一套 E3D 模型，二次加载同一规格构件时 `meshes` 请求数为 0（命中客户端 `MeshKey` 缓存）。

### Phase 4：会话级增量刷新（`increment_update`）与工程树面板（1.5 天）
1. 基于 `GeometryIdKey <-> Handle` 映射表，在 `consumer.rs` 中实现增量补丁应用（`apply_incremental_batch`）：
   - 先处理 `removed: Vec<GeometryIdKey>` 移除失效实体与实例；
   - 再处理 `upserted: Vec<E3dInstanceRecord>` 原地替换或新增实例（覆盖隐式管身随相邻管件位移后的自动重算）。
2. 在 UI 侧提供轻量「E3D 工程树 & 增量同步」面板：
   - 按需勾选 `SITE` / `ZONE` / `PIPE` 懒加载三维子树；
   - 展示 `Generation Ledger`（总账统计与未绘制隐式管身诊断提示）；
   - 一键触发或轮询会话增量更新（`increment_update`）。
3. **验收标准**：
   - 切换 Dabacon 会话窗口后，仅变更的构件与受牵连的 `ImpliedTube` 发生局部刷新，未变更构件句柄与相机姿态保持不变。

### Phase 5：`BevyBridge` M2 镜像层与大坐标分块消抖（2 天）
1. 推进 `src/scene/pipeline/bevy_bridge.rs` 的 Milestone 2（M2）：
   - 建立 `MeshKey -> bevy::asset::Handle<Mesh>` 共享资产表与 `GeometryIdKey -> bevy::ecs::entity::Entity` 映射；
   - 实现 **100m 空间分块原点重设（Spatial Chunk Origin Rebasing）**：在 `f64` 下将构件世界坐标拆分为 `chunk_origin_mm: IVec3`（\(10^5\text{ mm}\) 网格）+ 块内局部 `f32` 偏移，彻底消除厂区大坐标下的 WebGPU/wgpu `f32` 顶点抖动；
   - 按 `Noun` 映射工程 PBR 材质（低金属度、适度粗糙度，与管道厂区视觉规范对齐），选中态联动高亮，中心线/虚线继续由原生层借 Reversed-Z 深度缓冲合成。
2. **验收标准**：
   - `cargo run --features bevy3d` 下百万面级 E3D 区域流畅漫游，近距观察远离原点（\(>1\text{ km}\)）的法兰/管件无顶点抖动，选中高亮与虚线断管叠加准确。
