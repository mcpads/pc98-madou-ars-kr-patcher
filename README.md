# 마도물어 A.R.S (PC-98) 한글 패처

PC-98용 《마도물어 A.R.S》 플로피 디스크판 HDM 일곱 장에 한글 패치를 적용하는 Rust 코드입니다. HDM·FAT12 읽기와 쓰기, 오버레이 LZ 압축·해제, 메시지 카탈로그와 포인터 재배치, 컷신·DAT 텍스트 재삽입, V30 렌더러 훅, 한글 폰트 생성, 그래픽 리소스 컴파일, 파일 단위 BPS 패치 세트 생성을 제공합니다.

배포용 패치 세트와 적용 방법은 [마도물어 시리즈 한글 번역 프로젝트](https://github.com/mcpads/madou-monogatari-kr-patch/tree/main/pc98-madou-ars)에서 제공합니다.

## 제공하지 않는 것

이 저장소에는 원본 HDM, 패치를 적용한 HDM, 번역 JSON, 폰트 파일, 번역 그래픽 원화와 그 manifest가 없습니다. 따라서 이 저장소만으로는 배포 패치를 다시 만들 수 없습니다. 아래 입력을 직접 갖춘 경우에만 `build-release-set`이 패치 세트를 생성합니다.

## 빌드와 테스트

```bash
cargo build --release
cargo test
```

의존성 일부(`pc98-fat12-patcher-core`, `v30`)는 GitHub 저장소의 고정 커밋에서 받습니다.

기본 테스트는 합성 입력만 사용합니다. 원본 HDM, 번역, 폰트, 그래픽 원화나 디스크에서 꺼낸 파일이 필요한 테스트는 `#[ignore = "requires ..."]`로 필요한 입력을 밝혀 두었습니다. 입력을 갖춘 뒤 `cargo test -- --ignored`로 실행하며, 입력이 없으면 성공으로 넘어가지 않고 실패합니다.

| 테스트 입력 | 경로 |
| --- | --- |
| 원본 HDM 일곱 장과 User 디스크 | `roms/Madou Monogatari A.R.S [FD]/` (파일명은 테스트 소스 참고, `hdm_round_trip_alternate_dumps`는 `alts/`의 다른 덤프 16장도 필요) |
| 원본 HDI | `roms/Madou Monogatari A.R.S.hdi` |
| 디스크에서 꺼낸 파일 | `research/extracted/` (`extract-file`로 원본 HDM에서 추출) |
| 번역·폰트·그래픽 | 아래 빌드 입력과 같은 경로 |

## 지원 원본

플로피 디스크판의 헤더 없는 원본 HDM 일곱 장을 지원합니다. 모든 HDM의 크기는 1,261,568바이트이며, 빌드는 일곱 장의 크기와 SHA-256이 모두 일치해야 진행합니다. 기준값은 `assets/media/original_build_media.json`에 있습니다.

| 디스크 | SHA-256 |
| --- | --- |
| Demo | `185e1ea0e482f512971de4f41d41656c28b46f26cd69ea16e2a3362a10d6f56d` |
| Arle Game | `42113b22b028ce31c6319f301e29fdfc90ce27a91855d9e082547b1e229965fc` |
| Arle Data | `94d346dfdf793589f2630026770045ef6a25783112d896486e68d9d364e05281` |
| Rulue Game | `d629dc818533e9e0347fc83e0eab16cdd2388657636285c4c8d254ed087d2f89` |
| Rulue Data | `6e55fbaad1cf3b6f9ccaac54c09e86177cf2cd7350622e036599cf1754cdc428` |
| Schezo Game | `53c746d3c1bf8eaee16a96d9c29c2100e4b09ca405f3e57a1eb9059daedfb584` |
| Schezo Data | `fc12290819c86fd6fe8dd432d63aabcb6d40d93f94de0ced9de2b2c65666c489` |

## 빌드 입력

| 입력 | 기본 경로 | 비고 |
| --- | --- | --- |
| 번역 JSON | `assets/translations/complete/` | 오버레이 JSON과 `dat/`, `cutscene/`, `graphics/` 하위 디렉터리 (`--translations-dir`로 변경) |
| 원문 컷신 카탈로그 | `assets/translations/raw/cutscene/` | `--cutscene-raw-dir`로 변경 |
| 대사·컷신 폰트 | `assets/fonts/Galmuri14.ttf`, `assets/fonts/Galmuri-OFL.txt` | [Galmuri](https://github.com/quiple/galmuri) 2.403 |
| 데모 메뉴 폰트 | `assets/fonts/MaplestoryBold.ttf`, `assets/fonts/Maplestory-font-license.txt` | [메이플스토리 서체](https://maplestory.nexon.com/Media/Font) |
| 그래픽 manifest | `assets/graphics_text/*.json` | 타이틀 로고, 메뉴, 아르르 오프닝, 루루 막간 선택지, 루루 크레딧 (`--assets-dir`로 변경) |
| 그래픽 원화 | `assets/graphics_text/imagegen/PC98-DEMO-TITLE/master_rgb.png`, `assets/graphics_text/imagegen/PC98-ARLE-OPENING-ATSU/master_rgb.png` | manifest가 SHA-256을 고정 |

폰트 프로필 `assets/fonts/font_profile.json`과 `assets/fonts/maplestory_bold_menu_profile.json`은 저장소에 있습니다. 폰트 파일과 라이선스 문서는 재배포 조건을 이 저장소에서 보장할 수 없어 포함하지 않습니다. 각 폰트의 라이선스는 배포처에서 확인하세요. 프로필은 같은 디렉터리의 라이선스 파일이 없거나 폰트의 SHA-256이 다르면 빌드를 멈춥니다. 배포 패치 v1.0.1은 다음 폰트 파일로 만들었습니다.

```text
d3818c0f2898a3b2d79ccd04ec1e4de5e8940aa26abee261f73e315a44ce8df9  Galmuri14.ttf
d57eaff48a793ff872a0f33bba2943d058d07c81ed64c68054858a287b85811a  MaplestoryBold.ttf
```

## 패치 세트 생성

```bash
cargo run --release -- build-release-set \
  --demo-disk <Demo.hdm> \
  --arle-game-disk <Arle-Game.hdm> \
  --arle-data-disk <Arle-Data.hdm> \
  --rulue-game-disk <Rulue-Game.hdm> \
  --rulue-data-disk <Rulue-Data.hdm> \
  --schezo-game-disk <Schezo-Game.hdm> \
  --schezo-data-disk <Schezo-Data.hdm> \
  --output-dir out/release
```

출력 디렉터리는 아직 없어야 합니다. 일곱 디스크의 번역·폰트·재배치·그래픽 검사를 모두 통과하고 각 패키지를 원본에 다시 적용해 확인한 뒤에만 `madou-ars-kr-patch-<버전>.zip` 하나를 씁니다. 이 ZIP은 Demo와 캐릭터별 Game·Data 디스크용 패키지 일곱 개를 담은 RetroGame Patcher용 패치 세트입니다.

기본 번역 경로는 `complete` 상태만 받습니다. 배포 패치 v1.0.1은 `needs_review` 상태의 번역으로 만들었으므로, 같은 결과를 만들려면 다음 두 옵션을 더합니다.

```bash
--translations-dir assets/translations/needs_review --allow-needs-review
```

## 그 밖의 명령

`info`, `files`, `extract-file`, `validate-translation`, `glyph-stats`, `build-full-patch`, `apply-bps` 등의 사용법은 `cargo run -- help <명령>`으로 확인할 수 있습니다. 훅 검증용 일부 명령은 이 저장소에 없는 글리프 시트(`assets/gaiji/`)가 필요합니다.

## 라이선스

이 저장소의 소스 코드는 [MIT License](LICENSE)로 제공합니다.
