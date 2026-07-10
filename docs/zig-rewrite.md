# Zig 재작성 방향 검토

> 상태: 검토 노트 (의사결정 전)
> 대상: 현재 Rust 구현 (`src/`, ~1,700 LOC)

## TL;DR

현재 `mohyung`은 잘 정돈된 ~1,700줄 Rust 프로젝트로, 무게중심이 **성숙한 Rust 생태계(rayon · rusqlite · clap · anyhow)** 에 크게 실려 있다. Zig 재작성은:

- **정당화되는 경우**: (a) Zig 학습이 목적, (b) 크로스 컴파일/의존성 단순화를 최우선으로 두고 병렬 처리 코드를 직접 소유할 의향이 있음.
- **정당화되기 어려운 경우**: 순수하게 성능/제품 관점. 지금 코드에 Zig로 옮겨야 할 만큼의 결함이나 병목이 없고, rayon의 병렬성 편의를 직접 재구현하는 비용이 이득보다 크다.

**권장**: 전면 재작성 대신 **스파이크(spike)** 로 시작한다. `read → hash → compress → SQLite write` 코어만 Zig로 독립 포팅해 실제 `node_modules`에서 `pack`을 벤치마크하고, 데이터로 판단한다.

---

## 1. 현재 코드베이스가 기대는 것

| 영역 | 파일 | 핵심 의존성 |
|------|------|-------------|
| CLI 파싱 | `main.rs` | `clap` (derive) |
| 스캔 (병렬) | `core/scanner.rs` | `walkdir`, `rayon`, `serde_json` |
| 해싱 | `core/hasher.rs` | `sha2` |
| 압축 | `utils/compression.rs` | `flate2` (gzip) |
| 저장 | `core/store.rs` | `rusqlite` (bundled SQLite) |
| pack (병렬) | `commands/pack.rs` | `rayon`, `time`, `indicatif` |
| unpack (병렬) | `core/extractor.rs` | `rayon` |
| 에러 | 전역 | `anyhow` (`Context`, `with_context`) |

핵심: 이 프로젝트는 **I/O 바운드 + CPU 바운드가 섞인 배치 파이프라인**이고, 성능의 대부분이 `rayon` 기반 데이터 병렬성에서 나온다.

---

## 2. 의존성별 Zig 매핑

| Rust | Zig 대응 | 난이도 | 비고 |
|------|----------|--------|------|
| `sha2::Sha256` | `std.crypto.hash.sha2.Sha256` | 낮음 | stdlib에 있음, hex 포맷만 직접 |
| `serde_json` (읽기만) | `std.json` | 낮음 | `name`/`version`만 읽음 |
| `walkdir` | `std.fs.Dir.walk` | 낮음 | 심링크 처리만 주의 |
| `flate2` (gzip) | `std.compress.flate` | 중간 | API가 Zig 버전마다 변동 |
| `rusqlite` (bundled) | `@cImport("sqlite3.h")` + `sqlite3.c` | 중간 | C 직접 링크, **오히려 깔끔** |
| `clap` (derive) | 직접 파싱 또는 `zig-clap` | 중간 | 서브커맨드 3개뿐이라 감당 가능 |
| `indicatif` | 직접 구현 | 낮음 | 미관 요소 |
| `time` | `std.time` + 수동 포맷 | 낮음 | RFC3339 직접 작성 |
| `anyhow` (`Context`) | error union + diagnostic 구조체 | **높음** | 패러다임이 다름 (아래 참조) |
| `rayon` (`par_iter`) | `std.Thread.Pool` + 직접 큐/집계 | **높음** | **가장 큰 비용** (아래 참조) |

---

## 3. 가장 큰 리스크: rayon 병렬성

rayon은 세 곳의 핫스팟을 담당한다:

- `scanner.rs:313` — `package_dirs.par_iter().map(scan_package_files).collect()`
- `pack.rs:132` — `chunk.par_iter().map(|f| read + hash + compress)` (**가장 무거운 경로**)
- `extractor.rs:131` — `prepared.par_iter().map(write_file).try_reduce(...)`

rayon이 **공짜로** 주는 것:

- 작업 훔치기(work-stealing) 스케줄러, 자동 스레드 수 결정
- `collect::<Result<Vec<_>>>()` — 하나라도 `Err`면 조기 전파
- `try_reduce` — 병렬 집계 + 에러 단락(short-circuit)
- 데이터 경합 없음을 컴파일 타임에 보장

Zig에는 이에 대응하는 것이 없다. Zig 0.14의 `std.Thread.Pool` + `WaitGroup`으로 스레드 풀은 만들 수 있으나, **작업 큐, 결과 수집, 에러 집계/단락을 직접 구현**해야 한다. 여기에 얹혀 있는 원자적 진행률 카운터(`pack.rs:141`)와 배치 트랜잭션 경계까지 손으로 조립해야 한다.

이것이 재작성의 핵심 비용이다. 파이프라인 로직 자체는 단순하지만, "rayon 한 줄"이 "Zig 스레드풀 하니스 수십~수백 줄"로 늘어난다.

---

## 4. Zig가 실제로 주는 이점

**진짜 이점:**

1. **크로스 컴파일** — 현재 릴리스 매트릭스(`release.yml`)는 `aarch64-linux`만 `cross`(Docker)를 별도로 쓴다. bundled SQLite(C)가 크로스 컴파일을 까다롭게 만들기 때문이다. Zig는 `zig build -Dtarget=...` 한 줄로 한 머신에서 6개 타깃(+C 코드)을 모두 빌드한다. **이건 실질적인 승리.**
2. **SQLite C 직접 링크** — `rusqlite`의 "bundled"도 결국 C를 컴파일한다. Zig의 `@cImport`는 FFI 래퍼 크레이트 없이 SQLite를 1급으로 다룬다. `build.rs` 없음.
3. **바이너리/의존성 단순화** — Cargo 의존성 트리(현재 `Cargo.lock` ~28KB)가 사라진다.

**과대평가되기 쉬운 것:**

- **바이너리 크기/시작 속도** — Rust는 이미 `lto = true`, `codegen-units = 1`, `strip = true`로 작은 바이너리를 낸다. 개선 폭은 미미.
- **성능** — rayon을 잘 재현하지 못하면 오히려 **느려질** 수 있다. gzip/sha256은 두 언어 모두 비슷한 C/어셈블리 백엔드에 수렴.

---

## 5. 잃는 것

- **`cargo install mohyung` 배포 경로** — Zig는 crates.io 등가물이 없다. npm 래퍼(`npm/`)는 바이너리만 내려받으므로 영향 없음.
- **컴파일 타임 메모리 안전성** — Zig는 ReleaseSafe에서 런타임 체크는 있으나 빌림 검사기는 없다. 수동 `allocator` 관리 필요.
- **`anyhow` 스타일 에러 컨텍스트** — 현재 코드는 `.with_context(|| format!("failed to read {}", ...))`로 풍부한 에러 메시지를 낸다(`pack.rs:135`, `extractor.rs:136` 등). Zig error union은 **페이로드를 담지 못하므로**, 같은 UX를 원하면 diagnostic 구조체를 곁들이거나 throw 지점에서 직접 출력해야 한다. 방치하면 에러 메시지 품질이 하락.
- **언어 안정성** — Zig는 pre-1.0. `std.compress`, `std.json`, 빌드 시스템 API가 0.13→0.14→0.15에서 깨진다. 포팅 후에도 **Zig 릴리스 추적만으로 유지보수 부담**이 생긴다. Rust는 안정적.
- **테스트 자산** — 현재 `assert_cmd` 기반 E2E 통합 테스트(`tests/`)와 인라인 유닛 테스트를 Zig 테스트 하니스로 다시 작성.

---

## 6. 만약 진행한다면: 단계적 전략

전면 재작성을 한 번에 하지 말 것. 위험을 앞단으로 당긴다.

1. **스파이크 (1~2일)** — Zig로 `read → sha256 → gzip → SQLite insert` 코어만 독립 실행 파일로 작성. 실제 `node_modules`(수천~수만 파일)에서 `pack` 소요 시간을 Rust판과 비교. **여기서 병렬 하니스의 실제 비용과 성능이 드러난다.**
2. **병렬 하니스 확정** — 스파이크 결과가 좋으면, 재사용 가능한 `parMap(pool, items, fn) -> ![]Result` 유틸리티를 먼저 안정화. 이후 모든 포팅이 여기에 기댄다.
3. **모듈 단위 포팅** — 순수 함수부터: `hasher` → `compression` → `scanner` → `store` → 커맨드 순. 각 단계마다 동일 DB 스키마(schema v2)를 유지해 Rust판과 산출물 호환성 검증.
4. **골든 테스트** — 같은 `node_modules`를 Rust/Zig로 각각 pack → unpack → 결과 트리 diff가 0인지 확인.

DB 스키마(`store.rs`의 `CREATE_TABLES_SQL`)를 그대로 유지하면 Rust판으로 만든 `.db`를 Zig판으로 unpack할 수 있어, 점진적 검증이 가능하다.

---

## 7. 의사결정 체크리스트

전면 재작성을 **진행**하기 전에 아래에 "예"라고 답할 수 있어야 한다:

- [ ] 동기가 명확한가? (학습 / 크로스 컴파일 단순화 / 의존성 제거 중 하나 이상)
- [ ] rayon 병렬 하니스를 직접 만들고 유지할 의향이 있는가?
- [ ] pre-1.0 Zig의 API 변동을 계속 추적할 여력이 있는가?
- [ ] `cargo install` 경로 상실을 감수하는가?
- [ ] 스파이크 벤치마크에서 성능이 Rust판 대비 허용 범위인가?

하나라도 "아니오"라면, 현재 Rust 구현을 유지하는 것이 합리적이다. 지금 코드는 3개 플랫폼 CI, E2E 테스트, clippy 클린 상태로 이미 건강하다.

---

## 결론

기능/제품 관점만 보면 Zig 재작성은 **수평 이동**에 가깝다 — rayon의 편의를 손으로 재구현하고, 굳이 필요 없던 더 깔끔한 SQLite 링크를 얻는다. 다만 **크로스 컴파일 단순화**와 **의존성 트리 제거**는 실질적 이점이고, **Zig 학습**이 목적이라면 이 프로젝트는 규모(~1,700 LOC)와 성격(C 상호운용 + 병렬 배치)이 학습 소재로 매우 적합하다.

한 번에 베팅하지 말고 **스파이크 → 데이터 → 판단** 순서로 접근할 것을 권한다.
