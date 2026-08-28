# 자주 묻는 질문

## Ethereal이란 무엇인가요?

Ethereal은 ARM64 GKI 1.0 및 GKI 2.0용 커널 모듈 기반 루트 솔루션입니다. kernel Image를 다시 쓰지 않고 부팅 ramdisk에서 `ethereal.ko`를 로드합니다.

## 부팅 이미지 패치는 무엇을 변경하나요?

- GKI 1.0: `ethereal-init`, KO 및 기타 부팅 파일을 `boot.img` ramdisk에 추가하고, 같은 `boot.img`의 cmdline에 `rdinit=/ethereal-init`를 추가합니다.
- GKI 2.0: 파일은 `init_boot.img` ramdisk에 추가하고, `rdinit=/ethereal-init`는 짝이 되는 `boot.img` cmdline에 추가합니다. 따라서 두 이미지를 한 쌍으로 패치해야 합니다.

커널은 먼저 `/ethereal-init`를 실행합니다. 이 프로그램은 실행 중인 커널 release와 정확히 일치하는 KMI 모듈을 선택하고 `finit_module()`로 로드한 뒤 원본 `/init`를 실행합니다. Ethereal은 `/init`를 교체하거나 ELF 진입점을 변경하지 않습니다.

## 모든 커널에 사용할 수 있는 단일 KO가 없는 이유는 무엇인가요?

주 버전이 같은 커널도 Android KMI, 심볼 버전 및 CRC가 다를 수 있습니다. Ethereal은 지원하는 KMI마다 별도의 KO를 빌드하고 명확하게 일치하는 모듈만 로드합니다. 정확한 일치 항목이 없으면 Ethereal을 로드하지 않고 부팅을 계속합니다.
