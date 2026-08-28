# 자주 묻는 질문

## Ethereal이란 무엇인가요?

Ethereal은 ARM64 GKI 1.0 및 GKI 2.0용 커널 모듈 기반 루트 솔루션입니다. kernel Image를 다시 쓰지 않고 부팅 ramdisk에서 `ethereal.ko`를 로드합니다.

## 부팅 이미지 패치는 무엇을 변경하나요?

- GKI 1.0: `ethereal-init`, KO 및 기타 부팅 파일을 `boot.img` ramdisk에 추가하고, 같은 `boot.img`의 cmdline에 `rdinit=/ethereal-init`를 추가합니다.
- GKI 2.0 오프라인 패치: `init_boot.img` 하나만 선택합니다. 파일을 여기에 추가하고 원본 `/init`를 `init.ethereal.bak`으로 보관한 뒤, 추가 `PT_LOAD`가 ELF 진입점을 Ethereal 로더로 연결합니다. 짝이 되는 `boot.img`와 cmdline은 변경하지 않습니다. 커널만 든 GKI 2.0 `boot.img`는 단독 대상으로 거부합니다. Direct Install은 계속 `init_boot`와 `boot`를 하나의 트랜잭션으로 함께 패치합니다.

GKI 1.0과 Direct Install은 `rdinit`를 통해 `/ethereal-init`를 실행합니다. GKI 2.0 오프라인 경로는 원본 `/init`에 삽입된 로더로 진입해 정확한 KMI 모듈을 `finit_module()`로 로드한 뒤 원래 ELF 진입점으로 이동합니다. 원본 파일은 교체하지 않으며, 패치를 해제하면 `init.ethereal.bak`에서 복원합니다.

## 모든 커널에 사용할 수 있는 단일 KO가 없는 이유는 무엇인가요?

주 버전이 같은 커널도 Android KMI, 심볼 버전 및 CRC가 다를 수 있습니다. Ethereal은 지원하는 KMI마다 별도의 KO를 빌드하고 명확하게 일치하는 모듈만 로드합니다. 정확한 일치 항목이 없으면 Ethereal을 로드하지 않고 부팅을 계속합니다.
