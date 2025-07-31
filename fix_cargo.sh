#!/bin/bash

# 임시 작업 디렉토리 생성
TMP_DIR="$HOME/rustup_fix"
mkdir -p "$TMP_DIR"

# 현재 환경 변수 확인
echo "현재 PATH: $PATH"
echo "현재 RUSTUP_HOME: $RUSTUP_HOME"
echo "현재 CARGO_HOME: $CARGO_HOME"

# 임시로 RUSTUP_HOME 및 CARGO_HOME 설정
export RUSTUP_HOME="$TMP_DIR/.rustup"
export CARGO_HOME="$TMP_DIR/.cargo"
mkdir -p "$RUSTUP_HOME"
mkdir -p "$CARGO_HOME"

# 임시 환경에 Rust 설치
if [ ! -f "$CARGO_HOME/bin/cargo" ]; then
  echo "임시 환경에 Rust 설치 중..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
fi

# PATH 업데이트
export PATH="$CARGO_HOME/bin:$PATH"

# 빌드 실행
echo "cargo 버전 확인:"
cargo --version

# 소스 코드를 TMP_DIR로 복사
TMP_PROJECT_DIR="$TMP_DIR/project"
mkdir -p "$TMP_PROJECT_DIR"
echo "소스 코드를 임시 디렉토리로 복사 중..."
rsync -a --exclude target --exclude .git . "$TMP_PROJECT_DIR/"

# 임시 디렉토리로 이동하여 빌드
cd "$TMP_PROJECT_DIR"
echo "프로젝트 빌드 중... (디렉토리: $(pwd))"
cargo build

# 결과 상태 확인
if [ $? -eq 0 ]; then
    echo "빌드 성공!"
    # 빌드된 바이너리를 원래 디렉토리로 복사
    mkdir -p "$OLDPWD/target/debug"
    cp -f "$TMP_PROJECT_DIR/target/debug/cosmic-sync-server" "$OLDPWD/target/debug/" 2>/dev/null
    echo "빌드된 바이너리를 원래 디렉토리로 복사했습니다."
else
    echo "빌드 실패. 오류를 확인하세요."
fi

# 원래 디렉토리로 돌아가기
cd "$OLDPWD"

# 안내
echo ""
echo "이 스크립트를 다시 사용하려면 다음 명령어로 실행하세요:"
echo "bash fix_cargo.sh" 