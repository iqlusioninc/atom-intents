# Atom Intents Demo - Google Cloud Platform Deployment

This directory contains infrastructure and deployment configuration for running the Atom Intents demo as a public-facing service on Google Cloud Platform.

## Architecture

```
                                    ┌─────────────────────────────────────────┐
                                    │           Google Cloud Platform         │
                                    │                                         │
     Users                          │  ┌─────────────────────────────────┐   │
       │                            │  │      Cloud Load Balancer        │   │
       │                            │  │   (HTTPS + Cloud Armor WAF)     │   │
       ▼                            │  └──────────────┬──────────────────┘   │
  ┌─────────┐                       │                 │                       │
  │   DNS   │───────────────────────┼─────────────────┼───────────────────────┤
  └─────────┘                       │                 ▼                       │
                                    │  ┌─────────────────────────────────┐   │
                                    │  │         GKE Autopilot           │   │
                                    │  │  ┌───────────┐ ┌─────────────┐  │   │
                                    │  │  │  Web UI   │ │Skip Select  │  │   │
                                    │  │  │  (nginx)  │ │ Simulator   │  │   │
                                    │  │  └───────────┘ └──────┬──────┘  │   │
                                    │  └───────────────────────┼─────────┘   │
                                    │                          │              │
                                    │  ┌───────────────────────┼─────────┐   │
                                    │  │      Private Network  │         │   │
                                    │  │  ┌─────────────┐ ┌────┴──────┐  │   │
                                    │  │  │  Cloud SQL  │ │   Redis   │  │   │
                                    │  │  │ (PostgreSQL)│ │  (Cache)  │  │   │
                                    │  │  └─────────────┘ └───────────┘  │   │
                                    │  └─────────────────────────────────┘   │
                                    └─────────────────────────────────────────┘
```

## Components

| Component | Description |
|-----------|-------------|
| **GKE Autopilot** | Managed Kubernetes for running workloads |
| **Cloud SQL** | PostgreSQL for persistent storage |
| **Memorystore Redis** | Rate limiting and caching |
| **Cloud Load Balancer** | HTTPS termination with managed SSL |
| **Cloud Armor** | WAF with rate limiting and DDoS protection |
| **Cloud Build** | CI/CD pipeline |
| **Artifact Registry** | Container image storage |
| **Secret Manager** | Secure secrets storage |

## Prerequisites

1. **GCP Project** with billing enabled
2. **gcloud CLI** installed and authenticated
3. **Terraform** >= 1.5.0
4. **Docker** for building images
5. **kubectl** for Kubernetes management
6. **Domain** configured for the demo (e.g., demo.atom-intents.io)

## Quick Start

### 1. Set Environment Variables

```bash
export GCP_PROJECT_ID="your-project-id"
export GCP_REGION="us-central1"
```

### 2. Run Deployment Script

```bash
cd demo/gcp/scripts
chmod +x deploy.sh
./deploy.sh
```

This will:
- Create all GCP infrastructure via Terraform
- Build and push Docker images
- Deploy to GKE
- Configure load balancing and SSL

### 3. Configure DNS

Point your domain to the static IP output by the deployment:

```
demo.atom-intents.io -> <STATIC_IP>
```

## Manual Deployment

### Infrastructure (Terraform)

```bash
cd demo/gcp/terraform

# Initialize
terraform init

# Create terraform.tfvars
cat > terraform.tfvars <<EOF
project_id  = "your-project-id"
region      = "us-central1"
environment = "demo"
domain      = "demo.atom-intents.io"
EOF

# Plan and apply
terraform plan -out=tfplan
terraform apply tfplan
```

### Container Images

```bash
# Configure Docker
gcloud auth configure-docker us-central1-docker.pkg.dev

# Build Skip Select Simulator
docker build -t us-central1-docker.pkg.dev/PROJECT/atom-intents/skip-select-simulator:latest \
  -f demo/skip-select-simulator/Dockerfile demo/skip-select-simulator

# Build Web UI
docker build -t us-central1-docker.pkg.dev/PROJECT/atom-intents/atom-intents-web-ui:latest \
  -f demo/web-ui/Dockerfile demo/web-ui

# Push images
docker push us-central1-docker.pkg.dev/PROJECT/atom-intents/skip-select-simulator:latest
docker push us-central1-docker.pkg.dev/PROJECT/atom-intents/atom-intents-web-ui:latest
```

### Kubernetes Deployment

```bash
# Get credentials
gcloud container clusters get-credentials atom-intents-demo-gke --region us-central1

# Create secrets (copy from template first)
cp demo/gcp/k8s/secrets.yaml.template demo/gcp/k8s/secrets.yaml
# Edit secrets.yaml with actual values
kubectl apply -f demo/gcp/k8s/secrets.yaml

# Apply manifests
kubectl apply -f demo/gcp/k8s/
```

## CI/CD with Cloud Build

### Setup Trigger

```bash
gcloud builds triggers create github \
  --repo-name="atom-intents" \
  --repo-owner="your-org" \
  --branch-pattern="^main$" \
  --build-config="demo/gcp/cloudbuild.yaml"
```

### Manual Build

```bash
gcloud builds submit --config=demo/gcp/cloudbuild.yaml .
```

## Security Features

### Cloud Armor (WAF)

- **Rate Limiting**: 100 requests/minute per IP
- **XSS Protection**: Blocks cross-site scripting attempts
- **SQL Injection Protection**: Blocks SQL injection patterns
- **Bot Detection**: Adaptive protection against bots

### Application Security

- **HTTPS Only**: All traffic encrypted with TLS 1.2+
- **API Authentication**: API key-based auth for solvers
- **Rate Limiting**: Application-level rate limiting with Redis
- **Security Headers**: HSTS, X-Frame-Options, CSP, etc.

### Network Security

- **Private VPC**: Database and Redis on private network
- **Workload Identity**: No service account keys needed
- **Secret Manager**: Secure secrets management

## Monitoring

### Dashboards

Access the Grafana dashboard at:
- GKE → Workloads → skip-select → Metrics

Key metrics:
- Request rate and latency
- Error rates
- Active intents and auctions
- Settlement success rate

### Alerts

Configured alerts (see `monitoring.yaml`):
- High error rate (>5% for 5 minutes)
- High latency (p95 > 2 seconds)
- Pod not ready
- Low replica count
- Settlement failures

### Logs

```bash
# View Skip Select logs
kubectl logs -l app.kubernetes.io/name=skip-select-simulator -n atom-intents -f

# View in Cloud Logging
gcloud logging read 'resource.type="k8s_container" resource.labels.namespace_name="atom-intents"' --limit=100
```

## Cost Estimation

| Resource | Specification | Est. Monthly Cost |
|----------|---------------|-------------------|
| GKE Autopilot | ~2 vCPU, 4GB RAM | $50-80 |
| Cloud SQL | db-f1-micro | $10-15 |
| Redis | 1GB Basic | $30-40 |
| Load Balancer | + traffic | $20-30 |
| Cloud Armor | Standard tier | $5-10 |
| Artifact Registry | Storage | $1-5 |
| **Total** | | **~$120-180/month** |

*Note: Costs vary based on traffic and region. Use GCP pricing calculator for exact estimates.*

## Scaling

### Horizontal Pod Autoscaler

Skip Select and Web UI automatically scale based on:
- CPU utilization (target: 70%)
- Memory utilization (target: 80%)

Limits:
- Skip Select: 2-10 replicas
- Web UI: 2-5 replicas

### Database Scaling

For higher load:
1. Upgrade Cloud SQL tier (db-g1-small → db-custom-*)
2. Add read replicas
3. Enable high availability (REGIONAL)

## Troubleshooting

### Common Issues

**Pods not starting:**
```bash
kubectl describe pod -l app.kubernetes.io/name=skip-select-simulator -n atom-intents
kubectl logs -l app.kubernetes.io/name=skip-select-simulator -n atom-intents --previous
```

**Database connection issues:**
```bash
# Check Cloud SQL proxy
kubectl logs -l app.kubernetes.io/name=skip-select-simulator -n atom-intents | grep -i database
```

**SSL certificate pending:**
```bash
kubectl describe managedcertificate atom-intents-cert -n atom-intents
# May take up to 60 minutes for provisioning
```

### Rollback

```bash
kubectl rollout undo deployment/skip-select -n atom-intents
kubectl rollout undo deployment/web-ui -n atom-intents
```

## Cleanup

To destroy all resources:

```bash
# Delete Kubernetes resources
kubectl delete namespace atom-intents

# Destroy Terraform infrastructure
cd demo/gcp/terraform
terraform destroy
```

## Support

For issues with this deployment:
1. Check the troubleshooting section above
2. Review logs in Cloud Logging
3. Open an issue at https://github.com/iqlusioninc/atom-intents/issues
