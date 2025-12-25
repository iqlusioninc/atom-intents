#!/bin/bash
set -e

# ATOM Intents Demo - Google Cloud Deployment Script

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."

    if ! command -v gcloud &> /dev/null; then
        log_error "gcloud CLI not found. Install from: https://cloud.google.com/sdk/docs/install"
        exit 1
    fi

    if ! command -v docker &> /dev/null; then
        log_error "docker not found. Install from: https://docs.docker.com/get-docker/"
        exit 1
    fi

    if ! command -v kubectl &> /dev/null; then
        log_error "kubectl not found. Install from: https://kubernetes.io/docs/tasks/tools/"
        exit 1
    fi

    if ! command -v terraform &> /dev/null; then
        log_error "terraform not found. Install from: https://developer.hashicorp.com/terraform/downloads"
        exit 1
    fi
}

# Parse arguments
PROJECT_ID=""
REGION="us-central1"
ENVIRONMENT="dev"

while [[ $# -gt 0 ]]; do
    case $1 in
        --project)
            PROJECT_ID="$2"
            shift 2
            ;;
        --region)
            REGION="$2"
            shift 2
            ;;
        --env)
            ENVIRONMENT="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 --project <project-id> [--region <region>] [--env <environment>]"
            echo ""
            echo "Options:"
            echo "  --project  GCP project ID (required)"
            echo "  --region   GCP region (default: us-central1)"
            echo "  --env      Environment: dev, staging, prod (default: dev)"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [[ -z "$PROJECT_ID" ]]; then
    log_error "Project ID is required. Use --project <project-id>"
    exit 1
fi

check_prerequisites

log_info "Deploying ATOM Intents Demo"
log_info "  Project: $PROJECT_ID"
log_info "  Region:  $REGION"
log_info "  Environment: $ENVIRONMENT"

# Configure gcloud
log_info "Configuring gcloud..."
gcloud config set project "$PROJECT_ID"
gcloud auth configure-docker "${REGION}-docker.pkg.dev" --quiet

# Get Artifact Registry URL
REGISTRY_URL="${REGION}-docker.pkg.dev/${PROJECT_ID}/atom-intents-demo"

# Build and push images
log_info "Building and pushing container images..."

# Build Skip Select
log_info "Building Skip Select Simulator..."
docker build \
    -f "$SCRIPT_DIR/../docker/Dockerfile.skip-select" \
    -t "${REGISTRY_URL}/skip-select:latest" \
    "$SCRIPT_DIR/../skip-select-simulator"

docker push "${REGISTRY_URL}/skip-select:latest"

# Build Web UI
log_info "Building Web UI..."
docker build \
    -f "$SCRIPT_DIR/../docker/Dockerfile.web-ui" \
    -t "${REGISTRY_URL}/web-ui:latest" \
    "$SCRIPT_DIR/../web-ui"

docker push "${REGISTRY_URL}/web-ui:latest"

# Apply Terraform
log_info "Applying Terraform configuration..."
cd "$SCRIPT_DIR/terraform"

terraform init

terraform apply \
    -var="project_id=$PROJECT_ID" \
    -var="region=$REGION" \
    -var="environment=$ENVIRONMENT" \
    -auto-approve

# Get cluster credentials
CLUSTER_NAME=$(terraform output -raw cluster_name)
ZONE=$(terraform output -raw zone 2>/dev/null || echo "${REGION}-a")

log_info "Getting cluster credentials..."
gcloud container clusters get-credentials "$CLUSTER_NAME" --zone "$ZONE" --project "$PROJECT_ID"

# Deploy to Kubernetes
log_info "Deploying to Kubernetes..."
cd "$SCRIPT_DIR/k8s"

# Apply namespace
kubectl apply -f namespace.yaml

# Update image references and apply
sed "s|SKIP_SELECT_IMAGE|${REGISTRY_URL}/skip-select:latest|g" skip-select.yaml | kubectl apply -f -
sed "s|WEB_UI_IMAGE|${REGISTRY_URL}/web-ui:latest|g" web-ui.yaml | kubectl apply -f -

# Update ingress with correct IP name
sed "s|atom-intents-ip-dev|atom-intents-ip-${ENVIRONMENT}|g" ingress.yaml | kubectl apply -f -

# Wait for deployment
log_info "Waiting for deployments to be ready..."
kubectl rollout status deployment/skip-select -n atom-intents --timeout=300s
kubectl rollout status deployment/web-ui -n atom-intents --timeout=300s

# Get external IP
log_info "Getting external IP..."
EXTERNAL_IP=$(gcloud compute addresses describe "atom-intents-ip-${ENVIRONMENT}" --global --format='value(address)' 2>/dev/null || echo "pending")

echo ""
log_info "=========================================="
log_info "Deployment Complete!"
log_info "=========================================="
echo ""
log_info "Access the demo at:"
if [[ "$EXTERNAL_IP" != "pending" ]]; then
    log_info "  http://${EXTERNAL_IP}"
else
    log_info "  (IP pending, check: gcloud compute addresses describe atom-intents-ip-${ENVIRONMENT} --global)"
fi
echo ""
log_info "Useful commands:"
log_info "  kubectl get pods -n atom-intents"
log_info "  kubectl logs -f deployment/skip-select -n atom-intents"
log_info "  kubectl get ingress -n atom-intents"
