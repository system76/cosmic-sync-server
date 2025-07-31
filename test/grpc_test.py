#!/usr/bin/env python3
import grpc
import sys
import os
import time
import random
from concurrent import futures

# grpcio-tools로 생성된 프로토 파일의 경로
sys.path.append(os.path.abspath("../proto"))

try:
    import sync_pb2
    import sync_pb2_grpc
except ImportError:
    print("Proto 파일을 찾을 수 없습니다. 먼저 proto 파일을 생성해 주세요.")
    print("cd .. && python -m grpc_tools.protoc -I./proto --python_out=./proto --grpc_python_out=./proto ./proto/sync.proto")
    sys.exit(1)

def main():
    # gRPC 서버 주소
    server_address = "localhost:50051"
    
    # 계정 및 디바이스 해시 (테스트용)
    account_hash = "test_account_hash"
    device_hash = "test_device_hash"
    
    # 통신 채널 생성
    channel = grpc.insecure_channel(server_address)
    
    # SyncService 스텁 생성
    stub = sync_pb2_grpc.SyncServiceStub(channel)
    
    try:
        print(f"RegisterWatcherGroup 요청을 보냅니다...")
        
        # 워처 데이터 생성
        watcher_data = sync_pb2.WatcherData(
            folder="/home/test/folder",
            recursive_path=True,
            is_active=True,
            extra_json="{}"
        )
        
        # RegisterWatcherGroup 요청 생성
        group_id = int(time.time()) % 10000  # 현재 시간을 기반으로 한 그룹 ID
        request = sync_pb2.RegisterWatcherGroupRequest(
            account_hash=account_hash,
            device_hash=device_hash,
            group_id=group_id,
            title=f"Test Group {group_id}",
            watcher_data=watcher_data
        )
        
        # API 호출
        response = stub.RegisterWatcherGroup(request)
        
        # 응답 출력
        print(f"응답: success={response.success}, group_id={response.group_id}, message={response.return_message}")
        
    except grpc.RpcError as e:
        print(f"RPC 오류: {e.code()}: {e.details()}")
    except Exception as e:
        print(f"오류 발생: {e}")
    finally:
        # 채널 종료
        channel.close()

if __name__ == "__main__":
    main()

