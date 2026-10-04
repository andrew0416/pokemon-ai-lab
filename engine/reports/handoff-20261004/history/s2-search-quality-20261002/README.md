# S25b · 실제 포켓몬 국면의 탐색 품질 파일럿

결론: 첫 응답은 일부 빨라졌지만 큰 반격 누락과 메모리 상한 문제가 있어 **기본 OFF 유지**.
[RESULTS.md](RESULTS.md)부터 읽는다.

- 기준: c54becc9fb7bb9101cac363f389d4a84792016ab 위의 S25a 독립 사본.
- 게시 source: a0f7b556e30095e85ce56013c0e7d1f08206f03d.
- [GitHub run 36995572342](https://github.com/andrew0416/pokemon-ai-lab/actions/runs/36995572342): 성공, 3분 23초.
- 42 trial / 기준해 5개 / remote Rust 62검사 / Python 5검사.
- 감사: `AUDIT-36995572342.json`. raw artifacts: `publication/run-36995572342/`.
- 구현/고정계획/CI: `source/engine/benchmarks/s25_quality/`, `source/engine/search/src/budgeted*`.
- 로컬에서 성능을 측정하지 않았다. `validation/`은 bounded opt0 논리·clippy·build 근거다.

전체 repository는 재생 가능한 source snapshot이다. 생산 코드·원래 S25a source·KEEP-R1
registry와 기존 공유 Git checkout/index를 보존했다. 이 폴더는 C workspace `.gitignore`의
`/search-experiments/`로 제외되며, 검증한 파일 15개만 별도 실험 브랜치로 게시했다.
