# Integration test build comparison

기존 integration test 184개 실행 파일을 12개로 묶는 별도 빌드 실험이다.
원본은 P8g `693cd58eba1803827d18bb9d5276c5a05396fe43`이며 후보는 동일 엔진에서
scenario/search 테스트 설정 2개와 wrapper 8개만 바꾼다. 기존 테스트 본문과 모든
엔진 코드, 입력 자료, lock/dependency/profile은 보존한다. 생산 채택은 별도 판단이다.

scenario 171개 파일을 6개 모듈 묶음으로 만들고 `yy_position_hash`,
`ae_lab_check_timeout`, `lab_check_batch`, `factored`는 별도 실행 파일로 유지한다.
search의 9개 파일은 2개 묶음이 된다. 묶음은 원본 소스 bytes를 기준으로 균형을
맞췄으며 이는 실제 컴파일 비용의 측정값이 아니다. mapping.json에 전체 대응을 둔다.

`test-build-benchmark.yml`을 full baseline/candidate SHA로 수동 실행한다.
Python controller는 두 checkout을 별도 작업 디렉터리에 복사하며 원본을 수정하지
않는다. generic x86-64, 고정 Rust 1.98.1, release opt-level 3, codegen-units 16,
LTO off, incremental off, Cargo jobs 2를 양쪽에 동일하게 적용한다.

1. 처음 target이 없는 상태에서 두 버전을 컴파일한다. 의존성 다운로드는 준비 단계다.
2. Cargo가 실제 만든 테스트 실행 파일의 목록과 libtest `--list`를 수집한다.
3. 묶음 모듈 이름을 `(package, 원래 target, 테스트 함수)`로 정규화하여 누락·중복을 검사한다.
4. 원본/후보 모두 한 실행 파일 안에서는 `--test-threads=1`로 전부 실행하고,
   unit/bin/integration 테스트 및 별도 doc test 결과를 기록한다.
5. 두 작업 복사본의 core에 동일한 미사용 public const를 추가하고 기존 target으로
   재빌드하여 엔진 수정 후 상황을 비교한다. 다시 목록·개별 통과 결과를 대조한다.

컴파일 wall/CPU, 테스트 실행 시간, Cargo `--timings` HTML, 명령·stdout/stderr와
파일 해시를 보존한다. work copy와 target은 artifact 밖에 두며 로그/증거만 업로드한다.
실패·누락·추가·ignored·timeout을 성공으로 처리하지 않는다.

이것은 **단일 runner에서 수행하는 한 번의 cold 및 edited-core 비교**다. 순서와
runner 변동이 남으므로 반복 실험의 신뢰구간이나 엔진 실행 속도 향상으로 해석하지
않는다. 묶음 내부에서는 프로세스를 공유하므로 실제 실행 회귀가 필수다.

컨트롤러 검사는 Python 표준 라이브러리만 사용한다:

```sh
python -m unittest discover -s engine/benchmarks/test_bundles -p 'test_*.py' -v
```

참고: [Cargo integration targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests),
[Cargo build timings](https://doc.rust-lang.org/cargo/reference/timings.html).
