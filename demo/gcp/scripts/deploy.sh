#!/bin/bash
# Deploy Atom Intents Demo to GCP
# Usage: ./deploy.sh [environment]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Configuration
ENVIRONMENT="${1:-demo}"
PROJECT_ID="${GCP_PROJECT_ID:-}"
REGION="${GCP_REGION:-us-central1}"
CLUSTER_NAME="atom-intents-${ENVIRONMENT}-gke"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Validate requirements
check_requirements() {
    log_info "Checking requirements..."

    if [[ -z "$PROJECT_ID" ]]; then
        log_error "GCP_PROJECT_ID environment variable is required"
        exit 1
    fi

    for cmd in gcloud docker kubectl terraform; do
        if ! command -v "$cmd" &> /dev/null; then
            log_error "$cmd is required but not installed"
            exit 1
        fi
    done

    log_info "All requirements satisfied"
}

# Initialize Terraform
init_terraform() {
    log_info "Initializing Terraform..."
    cd "$PROJECT_ROOT/demo/gcp/terraform"

    # Create backend bucket if it doesn't exist
    if ! gsutil ls "gs://atom-intents-terraform-state" &> /dev/null; then
        log_info "Creating Terraform state bucket..."
        gsutil mb -p "$PROJECT_ID" -l "$REGION" "gs://atom-intents-terraform-state"
        gsutil versioning set on "gs://atom-intents-terraform-state"
    fi

    terraform init -upgrade
}

# Apply Terraform infrastructure
apply_infrastructure() {
    log_info "Applying Terraform infrastructure..."
    cd "$PROJECT_ROOT/demo/gcp/terraform"

    terraform plan \
        -var="project_id=$PROJECT_ID" \
        -var="region=$REGION" \
        -var="environment=$ENVIRONMENT" \
        -out=tfplan

    read -p "Apply infrastructure changes? (y/n) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        terraform apply tfplan
    else
        log_warn "Infrastructure changes skipped"
        return 1
    fi

    # Export outputs
    export REGISTRY=$(terraform output -raw artifact_registry)
    export DB_CONNECTION=$(terraform output -raw db_connection_name)
    export REDIS_HOST=$(terraform output -raw redis_host)
    export STATIC_IP=$(terraform output -raw static_ip)
}

# Build and push Docker images
build_images() {
    log_info "Building Docker images..."

    # Configure Docker for GCR
    gcloud auth configure-docker "$REGION-docker.pkg.dev" --quiet

    # Build Skip Select Simulator
    log_info "Building Skip Select Simulator..."
    docker build \
        -t "$REGISTRY/skip-select-simulator:latest" \
        -f "$PROJECT_ROOT/demo/skip-select-simulator/Dockerfile" \
        "$PROJECT_ROOT/demo/skip-select-simulator"

    # Build Web UI
    log_info "Building Web UI..."
    docker build \
        -t "$REGISTRY/atom-intents-web-ui:latest" \
        -f "$PROJECT_ROOT/demo/web-ui/Dockerfile" \
        "$PROJECT_ROOT/demo/web-ui"

    # Push images
    log_info "Pushing images to registry..."
    docker push "$REGISTRY/skip-select-simulator:latest"
    docker push "$REGISTRY/atom-intents-web-ui:latest"
}

# Deploy to Kubernetes
deploy_kubernetes() {
    log_info "Deploying to Kubernetes..."

    # Get cluster credentials
    gcloud container clusters get-credentials "$CLUSTER_NAME" \
        --region "$REGION" \
        --project "$PROJECT_ID"

    # Update manifests with actual values
    cd "$PROJECT_ROOT/demo/gcp/k8s"

    # Create temp directory for processed manifests
    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    for file in *.yaml; do
        if [[ "$file" == *".template"* ]]; then
            continue
        fi
        sed -e "s|REGISTRY|$REGISTRY|g" \
            -e "s|PROJECT_ID|$PROJECT_ID|g" \
            "$file" > "$TEMP_DIR/$file"
    done

    # Apply manifests
    kubectl apply -f "$TEMP_DIR/namespace.yaml"
    kubectl apply -f "$TEMP_DIR/configmap.yaml"
    kubectl apply -f "$TEMP_DIR/skip-select-deployment.yaml"
    kubectl apply -f "$TEMP_DIR/web-ui-deployment.yaml"
    kubectl apply -f "$TEMP_DIR/ingress.yaml"
    kubectl apply -f "$TEMP_DIR/monitoring.yaml"

    # Wait for deployments
    log_info "Waiting for deployments..."
    kubectl rollout status deployment/skip-select -n atom-intents --timeout=300s
    kubectl rollout status deployment/web-ui -n atom-intents --timeout=300s

    log_info "Deployment complete!"
}

# Print deployment info
print_info() {
    log_info "=== Deployment Summary ==="
    echo "Environment: $ENVIRONMENT"
    echo "Project: $PROJECT_ID"
    echo "Region: $REGION"
    echo "Cluster: $CLUSTER_NAME"
    echo ""
    echo "Access the demo at: https://demo.atom-intents.io"
    echo "(Note: DNS propagation may take a few minutes)"
    echo ""
    echo "Static IP: $STATIC_IP"
    echo "Configure DNS: demo.atom-intents.io -> $STATIC_IP"
}

# Main
main() {
    log_info "Starting Atom Intents Demo deployment..."

    check_requirements
    init_terraform
    apply_infrastructure
    build_images
    deploy_kubernetes
    print_info

    log_info "Deployment completed successfully!"
}

main "$@"
