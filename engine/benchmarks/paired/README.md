# GitHub Actions에서 엔진 성능 비교

`Actions → engine benchmark → Run workflow`에서 수동 실행한다.
기존 engine/search/oracle CI와 별도이며 push나 일정으로 벤치를 자동 시작하지 않는다.
공개 저장소의 표준 `ubuntu-24.04` 러너를 사용한다. 유료 larger runner와 로컬 PC는 쓰지 않는다.

## 입력

| 입력 | 의미 |
|---|---|
| baseline_sha | 저장소에 올라온 원본의 전체 40자리 커밋 SHA |
| candidate_sha | 후보의 전체 SHA. 비우면 선택한 실행 브랜치의 커밋 |
| candidate_feature | `none`(기본), `hurt-readers`(후보 P8g), `leaf-ending-states`(양쪽 P8g + 후보 P9), `prepared-turn`(양쪽 P8g + 후보 P8c), `compact-volatiles`(양쪽 P8g + 후보 P10) |
| suite | `smoke`: 작은 Harden/Poison Heal 국면의 동작 점검. `narrow`: coaching·sand 깊이 2 비교 |
| threads | 양쪽 동일 1/2/4스레드. 러너 가용 CPU 수를 넘으면 거부 |
| pairs | 각 국면에서 warmup을 제외한 쌍 수. 2/6/10/20, 기본 6 |

처음에는 같은 SHA를 양쪽에 넣고 `smoke`, 1스레드, 2쌍으로 실행한다.
이는 설치·실행·출력 대조 점검이다. 매우 짧은 smoke의 시간으로 성능을 판단하지 않는다.
이후 서로 다른 원본/후보 SHA와 `narrow`, 6쌍 이상으로 비교한다.
후보 코드가 로컬 실험 폴더에만 있으면 실행할 수 없다. 먼저 별도 후보 브랜치에 필요한
변경을 올려야 한다. 이 설정은 P8g/P8ha를 엔진에 채택하거나 두 후보를 결합하지 않는다.
보존된 P8g 후보를 측정할 때는 `candidate_feature=hurt-readers`를 명시한다.
원본·후보의 core manifest에는 동일한 빈 `experiment-hurt-readers = []` 선언이 있어야 하며,
default feature가 이를 직접 또는 간접으로 켜면 준비 단계에서 거부한다.
원본에는 추가 feature 플래그를 주지 않는다. 후보의 **모든** Cargo 테스트·빌드 명령에만
`--features lab-engine/experiment-hurt-readers`를 붙인다. `none`은 양쪽 모두 추가 플래그 없이 실행한다.

P9의 추가 효과는 `candidate_feature=leaf-ending-states`로 측정한다. 원본은
`--features lab-engine/experiment-hurt-readers`, 후보는
`--features lab-engine/experiment-hurt-readers,lab-search/experiment-leaf-ending-states`를 쓴다.
P8g 코드가 같은 두 커밋을 준비하고 원본에도 후보와 동일한 P9 feature 선언만 추가한다.
원본의 P9 코드는 활성화하지 않으며 Cargo manifest/lock 동일성 검사는 그대로 유지한다.
core/search의 P9 및 observer 선언·전달 관계와 default 비활성화를 검사한다.
두 버전의 core library/test 및 search library/test/공통 example 컴파일 지문을 보존하고,
양쪽 P8g on, 원본 P9 off, 후보 P9 on, 양쪽 observer off를 확인한다.
기존 평가기와 전체 출력 대조는 동일하다. 이 모드는 P8c와 결합하지 않는다.
이 timing 하네스는 CPU/wall 시간을 수집한다. P9 모드에는 peak RSS 단계가 없다.

P8c는 `candidate_feature=prepared-turn`으로 독립 비교한다. 원본은 P8g만,
후보는 `lab-engine/experiment-hurt-readers,lab-search/experiment-prepared-turn`을 켠다.
core/search의 prepared 및 observe feature 선언·전달 관계와 default 비활성화를 검사하고,
실제 library/test/example fingerprint에서 양쪽 P8g, 후보만 P8c, 양쪽 observe off를 확인한다.
원본에도 후보와 동일한 feature 선언만 넣으며 P9는 포함하지 않는다.
공통 하네스의 Config literal에는 prepared feature가 켜진 경우에만
`prepared_turn: true`를 명시한다. 다른 설정 필드는 이전과 같다. 이 feature 선언이 없는
과거 커밋을 비교할 때는 Rust의 `unexpected_cfgs` 경고가 날 수 있으나 해당 필드는 제외된다.
계측 기능이 필요한 후보의 새 `prepared_turn` 차등 테스트는 별도 target 디렉터리에서
observe on으로 검증한다. 이 실행 파일과 컴파일 결과는 성능 측정에 사용하지 않는다.

P8c 또는 P10의 CPU/wall 측정이 성공하면 `memory.py`로 별도의 peak RSS 검사를 수행한다.
동일한 바이너리·입력·검색 설정에서 각 국면 2쌍 AB/BA(총 4회, narrow 전체 8회)를 실행하고
GNU `/usr/bin/time`의 `%M` 값을 KiB 단위로 수집한다. 이 실행의 시간은 주 timing 통계에
합치지 않는다. 전체 프로세스의 peak RSS이므로 엔진 할당량이나 RSS 적분과 다르다.
타이밍 원자료의 성공·일정·파일 해시와 각 실행의 전체 출력 bits/작업량이 같아야 한다.
RSS pass에는 warmup을 두지 않으며, 작은 차이는 allocator/OS 변동을 포함해 해석한다.
`memory/` 아래 원자료·명령·해시·결과를 보존하고 실패 시에도 부분 결과를 남긴다.

P10은 `candidate_feature=compact-volatiles`, `suite=narrow`로 독립 비교한다.
양쪽 P8g를 켜고 후보만 `lab-engine/experiment-compact-volatiles`를 추가한다.
이 feature는 core의 빈 선언이며 search bridge가 없다. core와 search의 실제 컴파일
지문을 각각 검사하여 core에서 후보만 compact on, 양쪽 P9/P8c/observer off를 확인한다.
캐시 적중 여부와 관계없이 `compact_probe.py`가 원본·후보 on·후보 off를 세 개의
새 target에서 빌드하고, 동일한 56행 논리 JSONL을 byte 단위로 비교한다. off에서는
기존 `Copy` 및 공개 tuple API 검사도 실행한다. 타입 크기는 별도 `--layout` 결과이며
heap 사용량이나 RSS로 해석하지 않는다. probe와 off 검사 target은 측정용 target과 분리된다.

P10 비교용 두 커밋에는 CI4에서 검증한 동일 테스트 묶음을 적용한다. integration
실행 파일은 184→12개지만 기존 검사 본문은 유지한다. 묶음 내부 공유 상태의 영향을
제한하도록 P10의 전체 회귀는 `--test-threads=1`로 실행한다. compact 전용 추가 core
검사는 후보에서만 존재한다. smoke의 개별 target 이름과 혼동하지 않도록 이 모드는
narrow만 허용한다. 기본 엔진의 테스트 구성을 바꾸거나 P9/P8c를 합친 비교가 아니다.

## 비교 조건

두 버전을 별도 target 디렉터리에 준비하고 한 job의 같은 VM에서 측정한다.
아래 검증된 빌드 캐시가 적중한 경우 과거 빌드 산출물을 사용했다는 출처를 별도로 기록한다.
Rust 1.98.1, `cargo --locked`, generic x86-64(AVX2 필수 아님), 동일 release 설정을 쓴다.
Cargo.lock·각 package의 Cargo.toml·Cargo/toolchain 설정이 서로 다르면 의존성·빌드 설정 변화가 섞이므로 거부한다.
다른 Rust API를 쓰는 과거/미래 커밋은 공통 하네스 빌드가 실패할 수 있다.

`narrow`는 두 버전의 core/scenario/search 전체 테스트와 모든 빌드의 성공 증거를 확인한 뒤 측정한다.
캐시 적중 시 동일 조건의 과거 회귀 성공 증거를 재사용하며 현재 run에서 새로 실행한 테스트로 집계하지 않는다.
`smoke`는 설치 점검을 위해 core/search 라이브러리 테스트와 해당 oracle fixture가 속한
`abilities_slow_start_truant` 회귀 검사로 제한한다. 전체 회귀 통과로 해석하지 않는다.
첫 전체 검증은 빌드·테스트 때문에 수십 분 걸릴 수 있으며 측정 시간에는 포함하지 않는다.
각 버전의 target 디렉터리는 분리한다. 완성 빌드 캐시 적중 시에는 검증한 실행 파일과
컴파일 지문만 새 target에 복원한다. 적중하지 않으면 외부 의존성의 컴파일 산출물을
복원할 수 있으며, 이 경우 Cargo 빌드와 회귀 검사를 모두 현재 실행에서 수행한다.
빌드 뒤 `release/.fingerprint/lab-engine-*/lib-lab_engine.json`과
`test-lib-lab_engine.json` 원본·SHA·해석한 feature 목록을 artifact에 보존한다.
두 종류의 컴파일 지문이 모두 있어야 하며, 실험 feature가 원본에서는 없고
`hurt-readers` 후보에서는 실제로 활성화된 것을 확인해야 측정을 시작한다.
명령에 플래그를 적었다는 사실만으로 활성화를 인정하지 않는다.
공통 `harness.rs`를 각 checkout의 `engine/search/examples/ci_bench.rs`에 복사하여
동일 하네스를 빌드한다. 측정할 엔진 함수는 수정하지 않는다.
양쪽 모두 **controller(워크플로 실행 커밋)의 같은 시나리오와 팀 파일**을 읽는다.
원본/후보 코드의 dex는 각각의 커밋에 속하므로 엔진 데이터 변경도 후보 변경의 일부다.

- 국면별 양쪽 warmup 1회 제외, 이후 AB/BA 순서를 번갈아 같은 횟수로 실행한다.
- 모든 실행은 새 프로세스이며 동시에 두 벤치를 돌리지 않는다.
- setup·search 모두 Median, 깊이 2, beam 2/outcomes 2, Heuristic,
  factored off, split-heavy-cells false. 이번 설정은 PGO/SIMD 후보가 아니다.
- shallow/deep 행렬·전략·평가값을 float raw bits로 기록하고 계산 횟수도 엄격히 대조한다.
  각 검색 후 입력 State 복원을 assert한다. 불일치·오류·timeout은 실패한다.
- 같은 입력 반복의 일치는 **같은 작업을 측정했는지 확인하는 조건**이다.
  새로운 규칙 정확성 표본이 늘었다는 뜻이 아니다. 별도 회귀 테스트와 함께 해석한다.

`narrow`의 coaching은
`engine/oracle/scenarios/cc-lib-coaching-panda-vs-psy-sand-udon.json`의 가장 가능성 높은 초기 국면,
sand는 `engine/jobs/plan-20260926/gardevoir-vs-sand-owen.json`의 index 1이다.
sand 파일은 과거 로컬 P8g 입력과 팀 경로를 옮긴 동등 시나리오이며 파일 SHA 자체는 다르다.
이번 Linux/스레드 조건의 시간은 과거 Windows 1/6스레드 수치와 직접 비교하지 않는다.

## 결과 읽기

Actions 실행 요약과 `engine-benchmark-<run-id>-<attempt>` artifact를 확인한다.
artifact는 7일간 보관하므로 필요한 실험 결과는 만료 전에 내려받는다.
실패 시에도 생성된 요청·빌드 로그·원자료를 가능한 범위에서 업로드한다.
CPU/OS·Rust·커밋·입력·바이너리 해시, warmup과 각 실행의 stdout/stderr를 보존한다.
`request.json`에는 기능 선택, `test-plan.json`과 `provenance.json`에는 버전별 정확한 Cargo 명령을,
`fingerprints/`와 `<baseline|candidate>-features.json`에는 실제 컴파일 기능 증거를 남긴다.
CPU 시간은 Linux의 순차 child resource usage이며 wall 시간과 별도로 읽는다.

중앙값과 인접 쌍별 후보/원본 비율·범위를 함께 본다. 변동과 겹치는 작은 차이는 보류한다.
하나의 국면이나 실행의 결과를 엔진 전체의 개선으로 일반화하지 않는다.
hosted runner의 CPU 모델은 실행마다 바뀔 수 있으므로 다른 실행의 절대 시간을 직접
대조하지 않는다. 최종 채택 전 실제 사용할 PC에서도 확인한다.

한 작업은 최대 120분, 각 빌드·테스트 명령은 최대 2700초(45분), 개별 측정은 최대 600초다.
같은 워크플로는 한 번에 하나 실행한다.
동시 실행 요청이 쌓이면 GitHub concurrency 정책에 따라 오래된 대기 요청이 교체될 수 있다.
실행 중인 작업은 새 요청으로 취소하지 않는다. 저장소 공개 여부·요금 정책이 바뀌면
GitHub 사용량 설정도 다시 확인한다.

## 검증된 빌드 캐시

수동 실행의 `build_cache` 옵션을 끄면 기존의 새 빌드 경로를 사용한다. 켜면
원본·후보 각각의 완성된 `ci_bench` 실행 파일, 성공한 회귀 로그, 실제 Cargo feature
fingerprint와 출처 receipt를 묶어 보관한다. 전체 Cargo target 디렉터리나 성능 측정
원자료는 캐시하지 않는다. 최초 실행은 캐시를 채우므로 재빌드 비용이 그대로 발생한다.

이 완성 빌드 캐시는 **수정된 후보의 컴파일을 생략하는 기능이 아니다**. 동일 모드와
controller에서 후보만 바뀌면 고정된 원본의 cache key는 유지될 수 있지만, 후보의
소스 또는 빌드 조건이 달라지면 후보는 다시 빌드한다.

이를 보완하는 두 번째 층은 `Swatinem/rust-cache`의 **외부 의존성 캐시**다. 원본·후보
target을 분리하고 workspace crate, 도구 바이너리, incremental 산출물은 저장하지
않는다. 소스 SHA를 dependency key에 넣지 않아 엔진 소스만 수정한 뒤에도 변경 없는
라이브러리 컴파일을 재사용할 수 있다. toolchain, OS/arch, runner image, Cargo 설정과
profile/flags, manifest/lock, 모드와 suite는 key를 구분한다. 의존성 캐시의 prefix
복원은 빌드 보조 자료이며 완성 빌드의 exact-hit 증거로 취급하지 않는다.

의존성 캐시를 사용한 버전은 `reused=false`이며 전체 Cargo test/build와 feature 검사를
다시 수행한다. `*_DEPENDENCY_CACHE_READY=1`은 복원 action 완료 표식이며 실제 cache
다운로드 성공을 보증하지 않는다. helper는 외부 의존성 재사용을 목표로 workspace
fingerprint·테스트 실행 파일·링크가 섞인 target을 격리하지만 Cargo 지문의 정확성을
완전히 인증하지는 않는다. Cargo가 허용된 입력을 재검사한다.
완성 빌드 cache가 exact hit를 보고한 경우 의존성 복원은 생략하며,
그 완성 bundle이 검증에 실패하면 의존성 도움 없이 새로 빌드한다.

CI4의 단일 실험(run 36592430942)에서는 동일한 integration 테스트를 184→12개
실행 파일로 묶자 cold 빌드가 1144.020→162.454초, 기존 target을 유지한 core 수정 후
재빌드가 1141.786→158.681초로 줄었다. 양쪽 모두 동일 1238개 검사를 통과했다.
재빌드는 외부 의존성 산출물 33개를 재사용하면서도 모든 테스트 실행 파일을 다시
컴파일했다. 이 수치는 해당 빌드 구성의 관측이며 다른 수정·기기 또는 엔진 실행
속도의 향상률로 일반화하지 않는다.

복원은 GitHub의 exact cache-hit와 계산한 전체 key 일치가 모두 확인되고,
묶음 내부의 파일 목록·경로·해시·feature·회귀 성공 기록을 검증한 경우에만 허용한다.
소스와 빌드 조건을 담은 recipe가 다르거나 일부만 복원되면 새로 빌드한다.
입력에는 소스 commit/내용, 주입 하네스, toolchain·빌드 옵션·명령, 시스템 ABI와
controller 식별 정보가 포함된다. baseline/candidate를 따로 식별하고 observer 검증은
별도 target에서 매번 수행한다. 조건이 바뀐 바이너리를 재사용하지 않는다.

적중 시 과거 회귀 결과의 재사용 출처를 artifact에 기록한다. 시간·CPU·peak RSS와
그 출력·작업량 검사는 매번 새 프로세스로 수행한다. 한쪽만 적중해도 두 버전 모두
같은 CPU 범용 빌드 조건과 현재 VM의 warmup·측정 일정을 적용한다.

캐시 서비스의 장애·누락·손상은 새 빌드로 대체한다. 실제 컴파일이나 테스트 실패는
실패로 처리한다. 저장은 신뢰된 저장소의 `lab-engine`에서 수동 실행하고 빌드·회귀·
feature 검증과 봉인이 끝난 묶음에만 허용한다. 파일 해시는 손상 검사이며 작성자 인증
서명이 아니다. cache 서비스의 저장소·브랜치 권한 경계를 전제로 한다.

GitHub 캐시는 동일 key를 덮어쓸 수 없으므로 손상된 exact hit는 해당 실행에서 새로
빌드해 사용해도 자동으로 원격 캐시를 고치지 않는다. 또한 branch scope와 만료·퇴거로
인해 항상 적중하는 것은 아니다. 캐시 용량 설정이나 유료 한도를 변경하지 않는다.

근거: [GitHub cache 동작과 접근 범위](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching),
[Cargo build cache 구조](https://doc.rust-lang.org/cargo/reference/build-cache.html),
[rust-cache 범위](https://github.com/Swatinem/rust-cache),
[Cargo build timings](https://doc.rust-lang.org/cargo/reference/timings.html).

## 컨트롤러 검사

```sh
python -m unittest discover -s engine/benchmarks/paired -p 'test_*.py' -v
```

Python 표준 라이브러리만 필요하다. 테스트는 가짜 실행 파일로 실패·timeout·순서·불일치를
검사하며 Rust 성능 측정을 하지 않는다.
