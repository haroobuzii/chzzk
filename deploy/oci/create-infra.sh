#!/usr/bin/env bash
# OCI에 치지직 배포 서버(Always Free)를 만든다. ADR 0008, docs/deploy-notes.md
#
# 준비: OCI CLI 설치와 ~/.oci/config(API 키), 서버 접속용 SSH 공개 키.
# 쓰는 법: deploy/oci/create-infra.sh [SSH 공개 키 경로]   (기본 ~/.ssh/chzzk_oci.pub)
#
# 만드는 것(모두 Always Free 범위):
#   - VCN chzzk-vcn(10.0.0.0/16), 인터넷 게이트웨이, 공개 서브넷(10.0.0.0/24)
#   - 보안 목록: TCP 22(SSH)·80(HTTP)·443(HTTPS), UDP 8189(라이브 영상 WebRTC) 들어옴 허용
#   - VM chzzk: VM.Standard.A1.Flex 2 OCPU·12 GB, Ubuntu 24.04 aarch64, 부트 볼륨 100 GB, 공인 IP
# 이미 있는 것은 다시 만들지 않는다. 끝나면 공인 IP를 출력한다.
set -euo pipefail
export SUPPRESS_LABEL_WARNING=True

SSH_KEY="${1:-$HOME/.ssh/chzzk_oci.pub}"
NAME=chzzk
SHAPE=VM.Standard.A1.Flex
OCPUS=2
MEMORY_GB=12
BOOT_GB=100
HERE="$(cd "$(dirname "$0")" && pwd)"

TENANCY=$(awk -F= '/^tenancy=/{print $2}' ~/.oci/config)
C="$TENANCY"   # 루트 컴파트먼트에 만든다(리소스가 적어 따로 나누지 않는다).

q() { oci "$@" 2>/dev/null; }

AD=$(q iam availability-domain list -c "$TENANCY" --query 'data[0].name' --raw-output)
echo "가용 영역: $AD"

VCN=$(q network vcn list -c "$C" --display-name "$NAME-vcn" --query 'data[0].id' --raw-output || true)
if [ -z "$VCN" ] || [ "$VCN" = "null" ]; then
  VCN=$(q network vcn create -c "$C" --display-name "$NAME-vcn" --cidr-blocks '["10.0.0.0/16"]' --dns-label chzzk --wait-for-state AVAILABLE --query 'data.id' --raw-output)
  echo "VCN 만듦"
fi

IGW=$(q network internet-gateway list -c "$C" --vcn-id "$VCN" --query 'data[0].id' --raw-output || true)
if [ -z "$IGW" ] || [ "$IGW" = "null" ]; then
  IGW=$(q network internet-gateway create -c "$C" --vcn-id "$VCN" --is-enabled true --display-name "$NAME-igw" --wait-for-state AVAILABLE --query 'data.id' --raw-output)
  echo "인터넷 게이트웨이 만듦"
fi

RT=$(q network vcn get --vcn-id "$VCN" --query 'data."default-route-table-id"' --raw-output)
q network route-table update --rt-id "$RT" --force \
  --route-rules "[{\"destination\":\"0.0.0.0/0\",\"destinationType\":\"CIDR_BLOCK\",\"networkEntityId\":\"$IGW\"}]" >/dev/null
echo "기본 경로: 인터넷 게이트웨이"

SL=$(q network vcn get --vcn-id "$VCN" --query 'data."default-security-list-id"' --raw-output)
q network security-list update --security-list-id "$SL" --force \
  --egress-security-rules '[{"destination":"0.0.0.0/0","protocol":"all","isStateless":false}]' \
  --ingress-security-rules '[
    {"source":"0.0.0.0/0","protocol":"6","isStateless":false,"tcpOptions":{"destinationPortRange":{"min":22,"max":22}},"description":"SSH"},
    {"source":"0.0.0.0/0","protocol":"6","isStateless":false,"tcpOptions":{"destinationPortRange":{"min":80,"max":80}},"description":"HTTP (Let'"'"'s Encrypt)"},
    {"source":"0.0.0.0/0","protocol":"6","isStateless":false,"tcpOptions":{"destinationPortRange":{"min":443,"max":443}},"description":"HTTPS"},
    {"source":"0.0.0.0/0","protocol":"17","isStateless":false,"udpOptions":{"destinationPortRange":{"min":8189,"max":8189}},"description":"MediaMTX WebRTC"},
    {"source":"0.0.0.0/0","protocol":"1","isStateless":false,"icmpOptions":{"type":3,"code":4}}
  ]' >/dev/null
echo "보안 목록: TCP 22·80·443, UDP 8189"

SUBNET=$(q network subnet list -c "$C" --vcn-id "$VCN" --display-name "$NAME-public" --query 'data[0].id' --raw-output || true)
if [ -z "$SUBNET" ] || [ "$SUBNET" = "null" ]; then
  SUBNET=$(q network subnet create -c "$C" --vcn-id "$VCN" --cidr-block 10.0.0.0/24 --display-name "$NAME-public" --dns-label public --wait-for-state AVAILABLE --query 'data.id' --raw-output)
  echo "공개 서브넷 만듦"
fi

INSTANCE=$(q compute instance list -c "$C" --display-name "$NAME" --lifecycle-state RUNNING --query 'data[0].id' --raw-output || true)
if [ -z "$INSTANCE" ] || [ "$INSTANCE" = "null" ]; then
  IMAGE=$(q compute image list -c "$C" --operating-system "Canonical Ubuntu" --operating-system-version "24.04" --shape "$SHAPE" --sort-by TIMECREATED --sort-order DESC --query 'data[?!contains("display-name", `Minimal`)] | [0].id' --raw-output)
  echo "VM 만드는 중($SHAPE ${OCPUS} OCPU·${MEMORY_GB} GB)… 재고가 없으면 'Out of host capacity' 오류가 난다."
  INSTANCE=$(oci compute instance launch -c "$C" --availability-domain "$AD" --display-name "$NAME" \
    --shape "$SHAPE" --shape-config "{\"ocpus\":$OCPUS,\"memoryInGBs\":$MEMORY_GB}" \
    --image-id "$IMAGE" --boot-volume-size-in-gbs "$BOOT_GB" \
    --subnet-id "$SUBNET" --assign-public-ip true \
    --ssh-authorized-keys-file "$SSH_KEY" --user-data-file "$HERE/cloud-init.yaml" \
    --wait-for-state RUNNING --query 'data.id' --raw-output)
  echo "VM 만듦"
fi

IP=$(q compute instance list-vnics --instance-id "$INSTANCE" --query 'data[0]."public-ip"' --raw-output)
echo "공인 IP: $IP"
