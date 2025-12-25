#!/bin/bash
set -e

# ATOM Intents Demo - Google Cloud Initial Setup Script
# Run this once to set up the GCP project for the demo

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Parse arguments
PROJECT_ID=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --project)
            PROJECT_ID="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 --project <project-id>"
            echo ""
            echo "Sets up a GCP project for the ATOM Intents demo."
            echo "This script should be run once before deploying."
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

log_info "Setting up GCP project: $PROJECT_ID"

# Configure gcloud
gcloud config set project "$PROJECT_ID"

# Enable required APIs
log_info "Enabling required APIs..."
gcloud services enable \
    container.googleapis.com \
    compute.googleapis.com \
    cloudresourcemanager.googleapis.com \
    iam.googleapis.com \
    logging.googleapis.com \
    monitoring.googleapis.com \
    artifactregistry.googleapis.com \
    run.googleapis.com \
    sqladmin.googleapis.com \
    redis.googleapis.com

# Create Terraform state bucket
STATE_BUCKET="atom-intents-terraform-state"
log_info "Creating Terraform state bucket: $STATE_BUCKET"

if gsutil ls "gs://${STATE_BUCKET}" 2>/dev/null; then
    log_warn "Bucket already exists"
else
    gsutil mb -l us-central1 "gs://${STATE_BUCKET}"
    gsutil versioning set on "gs://${STATE_BUCKET}"
fi

# Create Artifact Registry repository
log_info "Creating Artifact Registry repository..."
gcloud artifacts repositories create atom-intents-demo \
    --repository-format=docker \
    --location=us-central1 \
    --description="ATOM Intents Demo container images" \
    2>/dev/null || log_warn "Repository already exists"

echo ""
log_info "=========================================="
log_info "Setup Complete!"
log_info "=========================================="
echo ""
log_info "Next steps:"
log_info "  1. cd $SCRIPT_DIR/terraform"
log_info "  2. cp terraform.tfvars.example terraform.tfvars"
log_info "  3. Edit terraform.tfvars with your project settings"
log_info "  4. Run: ./deploy.sh --project $PROJECT_ID"
