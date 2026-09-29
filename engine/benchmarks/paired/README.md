# GitHub Actions에서 엔진 성능 비교

`Actions → engine benchmark → Run workflow`에서 수동 실행한다.
기존 engine/search/oracle CI와 별도이며 push나 일정으로 벤치를 자동 시작하지 않는다.
공개 저장소의 표준 `ubuntu-24.04` 러너를 사용한다. 유료 larger runner와 로컬 PC는 쓰지 않는다.

## 입력

| 입력 | 의미 |
|---|---|
| baseline_sha | 저장소에 올라온 원본의 전체 40자리 커밋 SHA |
| candidate_sha | 후보의 전체 SHA. 비우면 선택한 실행 브랜치의 커밋 |
| candidate_feature | `none`(기본) 또는 `hurt-readers`. 후보에만 `lab-engine/experiment-hurt-readers`를 활성화 |
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

## 비교 조건

한 job의 같은 VM에서 두 버전을 별도 target 디렉터리에 빌드한다.
Rust 1.98.1, `cargo --locked`, generic x86-64(AVX2 필수 아님), 동일 release 설정을 쓴다.
Cargo.lock·각 package의 Cargo.toml·Cargo/toolchain 설정이 서로 다르면 의존성·빌드 설정 변화가 섞이므로 거부한다.
다른 Rust API를 쓰는 과거/미래 커밋은 공통 하네스 빌드가 실패할 수 있다.

`narrow`는 두 버전의 core/scenario/search 전체 테스트와 모든 빌드가 성공한 뒤 측정한다.
`smoke`는 설치 점검을 위해 core/search 라이브러리 테스트와 해당 oracle fixture가 속한
`abilities_slow_start_truant` 회귀 검사로 제한한다. 전체 회귀 통과로 해석하지 않는다.
첫 전체 검증은 빌드·테스트 때문에 수십 분 걸릴 수 있으며 측정 시간에는 포함하지 않는다.
각 버전의 target 디렉터리는 새로 만들어 이전 컴파일 결과와 섞이지 않게 한다.
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

## 컨트롤러 검사

```sh
python -m unittest discover -s engine/benchmarks/paired -p 'test_*.py' -v
```

Python 표준 라이브러리만 필요하다. 테스트는 가짜 실행 파일로 실패·timeout·순서·불일치를
검사하며 Rust 성능 측정을 하지 않는다.
