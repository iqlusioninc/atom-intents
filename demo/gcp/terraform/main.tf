# Atom Intents Demo - Google Cloud Platform Infrastructure
# Public-facing deployment with GKE, Cloud SQL, and Load Balancing

terraform {
  required_version = ">= 1.5.0"

  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 5.0"
    }
    google-beta = {
      source  = "hashicorp/google-beta"
      version = "~> 5.0"
    }
    kubernetes = {
      source  = "hashicorp/kubernetes"
      version = "~> 2.23"
    }
  }

  # Using local backend for new project deployment
  # backend "gcs" {
  #   bucket = "atom-intents-terraform-state"
  #   prefix = "demo"
  # }
}

# Variables
variable "project_id" {
  description = "GCP Project ID"
  type        = string
}

variable "region" {
  description = "GCP Region"
  type        = string
  default     = "us-central1"
}

variable "environment" {
  description = "Environment name (dev, staging, prod)"
  type        = string
  default     = "demo"
}

variable "domain" {
  description = "Domain name for the demo"
  type        = string
  default     = "demo.atom-intents.io"
}

# Provider configuration
provider "google" {
  project = var.project_id
  region  = var.region
}

provider "google-beta" {
  project = var.project_id
  region  = var.region
}

# Locals
locals {
  project_name = "atom-intents-demo"
  labels = {
    project     = "atom-intents"
    environment = var.environment
    managed_by  = "terraform"
  }
}

# Enable required APIs
resource "google_project_service" "apis" {
  for_each = toset([
    "container.googleapis.com",
    "sqladmin.googleapis.com",
    "secretmanager.googleapis.com",
    "cloudresourcemanager.googleapis.com",
    "compute.googleapis.com",
    "servicenetworking.googleapis.com",
    "redis.googleapis.com",
    "monitoring.googleapis.com",
    "logging.googleapis.com",
    "cloudbuild.googleapis.com",
    "artifactregistry.googleapis.com",
  ])

  project = var.project_id
  service = each.value

  disable_on_destroy = false
}

# VPC Network
resource "google_compute_network" "main" {
  name                    = "${local.project_name}-vpc"
  project                 = var.project_id
  auto_create_subnetworks = false

  depends_on = [google_project_service.apis]
}

resource "google_compute_subnetwork" "main" {
  name          = "${local.project_name}-subnet"
  project       = var.project_id
  region        = var.region
  network       = google_compute_network.main.id
  ip_cidr_range = "10.0.0.0/20"

  secondary_ip_range {
    range_name    = "pods"
    ip_cidr_range = "10.1.0.0/16"
  }

  secondary_ip_range {
    range_name    = "services"
    ip_cidr_range = "10.2.0.0/20"
  }

  private_ip_google_access = true
}

# Cloud NAT for outbound internet access
resource "google_compute_router" "main" {
  name    = "${local.project_name}-router"
  project = var.project_id
  region  = var.region
  network = google_compute_network.main.id
}

resource "google_compute_router_nat" "main" {
  name                               = "${local.project_name}-nat"
  project                            = var.project_id
  router                             = google_compute_router.main.name
  region                             = var.region
  nat_ip_allocate_option             = "AUTO_ONLY"
  source_subnetwork_ip_ranges_to_nat = "ALL_SUBNETWORKS_ALL_IP_RANGES"
}

# Private service access for Cloud SQL
resource "google_compute_global_address" "private_ip" {
  name          = "${local.project_name}-private-ip"
  project       = var.project_id
  purpose       = "VPC_PEERING"
  address_type  = "INTERNAL"
  prefix_length = 16
  network       = google_compute_network.main.id
}

resource "google_service_networking_connection" "private_vpc" {
  network                 = google_compute_network.main.id
  service                 = "servicenetworking.googleapis.com"
  reserved_peering_ranges = [google_compute_global_address.private_ip.name]
}

# GKE Cluster
resource "google_container_cluster" "main" {
  name     = "${local.project_name}-gke"
  project  = var.project_id
  location = var.region

  # Use regional cluster for high availability
  node_locations = [
    "${var.region}-a",
    "${var.region}-b",
  ]

  network    = google_compute_network.main.name
  subnetwork = google_compute_subnetwork.main.name

  # Enable Autopilot for simplified management
  enable_autopilot = true

  ip_allocation_policy {
    cluster_secondary_range_name  = "pods"
    services_secondary_range_name = "services"
  }

  # Private cluster configuration
  private_cluster_config {
    enable_private_nodes    = true
    enable_private_endpoint = false
    master_ipv4_cidr_block  = "172.16.0.0/28"
  }

  # Master authorized networks
  master_authorized_networks_config {
    cidr_blocks {
      cidr_block   = "0.0.0.0/0"
      display_name = "All networks"
    }
  }

  # Workload Identity
  workload_identity_config {
    workload_pool = "${var.project_id}.svc.id.goog"
  }

  # Monitoring and logging
  logging_config {
    enable_components = ["SYSTEM_COMPONENTS", "WORKLOADS"]
  }

  monitoring_config {
    enable_components = ["SYSTEM_COMPONENTS"]
    managed_prometheus {
      enabled = true
    }
  }

  resource_labels = local.labels

  depends_on = [
    google_project_service.apis,
    google_service_networking_connection.private_vpc,
  ]
}

# Cloud SQL PostgreSQL Instance
resource "google_sql_database_instance" "main" {
  name             = "${local.project_name}-postgres"
  project          = var.project_id
  region           = var.region
  database_version = "POSTGRES_15"

  settings {
    tier              = "db-f1-micro"  # Use larger tier for production
    availability_type = "ZONAL"        # Use REGIONAL for production HA
    disk_size         = 10
    disk_type         = "PD_SSD"

    ip_configuration {
      ipv4_enabled    = false
      private_network = google_compute_network.main.id
    }

    backup_configuration {
      enabled                        = true
      start_time                     = "03:00"
      point_in_time_recovery_enabled = true
      backup_retention_settings {
        retained_backups = 7
      }
    }

    maintenance_window {
      day  = 7  # Sunday
      hour = 3
    }

    database_flags {
      name  = "max_connections"
      value = "100"
    }

    user_labels = local.labels
  }

  deletion_protection = false  # Set to true for production

  depends_on = [google_service_networking_connection.private_vpc]
}

resource "google_sql_database" "main" {
  name     = "atom_intents"
  project  = var.project_id
  instance = google_sql_database_instance.main.name
}

resource "google_sql_user" "main" {
  name     = "atom_intents_app"
  project  = var.project_id
  instance = google_sql_database_instance.main.name
  password = random_password.db_password.result
}

resource "random_password" "db_password" {
  length  = 32
  special = false
}

# Redis for caching and rate limiting
resource "google_redis_instance" "main" {
  name           = "${local.project_name}-redis"
  project        = var.project_id
  region         = var.region
  memory_size_gb = 1
  tier           = "BASIC"  # Use STANDARD_HA for production

  authorized_network = google_compute_network.main.id
  connect_mode       = "PRIVATE_SERVICE_ACCESS"

  redis_version = "REDIS_7_0"

  labels = local.labels

  depends_on = [google_service_networking_connection.private_vpc]
}

# Artifact Registry for container images
resource "google_artifact_registry_repository" "main" {
  project       = var.project_id
  location      = var.region
  repository_id = "atom-intents"
  format        = "DOCKER"

  labels = local.labels

  depends_on = [google_project_service.apis]
}

# Secret Manager secrets
resource "google_secret_manager_secret" "db_password" {
  project   = var.project_id
  secret_id = "${local.project_name}-db-password"

  replication {
    auto {}
  }

  labels = local.labels

  depends_on = [google_project_service.apis]
}

resource "google_secret_manager_secret_version" "db_password" {
  secret      = google_secret_manager_secret.db_password.id
  secret_data = random_password.db_password.result
}

resource "google_secret_manager_secret" "api_keys" {
  project   = var.project_id
  secret_id = "${local.project_name}-api-keys"

  replication {
    auto {}
  }

  labels = local.labels
}

# Service Account for workloads
resource "google_service_account" "app" {
  project      = var.project_id
  account_id   = "${local.project_name}-app"
  display_name = "Atom Intents Demo Application"
}

resource "google_project_iam_member" "app_cloudsql" {
  project = var.project_id
  role    = "roles/cloudsql.client"
  member  = "serviceAccount:${google_service_account.app.email}"
}

resource "google_project_iam_member" "app_secretmanager" {
  project = var.project_id
  role    = "roles/secretmanager.secretAccessor"
  member  = "serviceAccount:${google_service_account.app.email}"
}

resource "google_project_iam_member" "app_logging" {
  project = var.project_id
  role    = "roles/logging.logWriter"
  member  = "serviceAccount:${google_service_account.app.email}"
}

resource "google_project_iam_member" "app_monitoring" {
  project = var.project_id
  role    = "roles/monitoring.metricWriter"
  member  = "serviceAccount:${google_service_account.app.email}"
}

# Workload Identity binding
resource "google_service_account_iam_member" "workload_identity" {
  service_account_id = google_service_account.app.name
  role               = "roles/iam.workloadIdentityUser"
  member             = "serviceAccount:${var.project_id}.svc.id.goog[atom-intents/skip-select]"
}

# Static IP for Load Balancer
resource "google_compute_global_address" "main" {
  name    = "${local.project_name}-ip"
  project = var.project_id
}

# Cloud Armor security policy
resource "google_compute_security_policy" "main" {
  name    = "${local.project_name}-security-policy"
  project = var.project_id

  # Default rule - allow
  rule {
    action   = "allow"
    priority = "2147483647"
    match {
      versioned_expr = "SRC_IPS_V1"
      config {
        src_ip_ranges = ["*"]
      }
    }
    description = "Default allow rule"
  }

  # Rate limiting rule
  rule {
    action   = "rate_based_ban"
    priority = "1000"
    match {
      versioned_expr = "SRC_IPS_V1"
      config {
        src_ip_ranges = ["*"]
      }
    }
    rate_limit_options {
      conform_action = "allow"
      exceed_action  = "deny(429)"
      rate_limit_threshold {
        count        = 100
        interval_sec = 60
      }
      ban_duration_sec = 600
    }
    description = "Rate limiting - 100 requests per minute"
  }

  # Block common attack patterns
  rule {
    action   = "deny(403)"
    priority = "100"
    match {
      expr {
        expression = "evaluatePreconfiguredExpr('xss-v33-stable')"
      }
    }
    description = "Block XSS attacks"
  }

  rule {
    action   = "deny(403)"
    priority = "101"
    match {
      expr {
        expression = "evaluatePreconfiguredExpr('sqli-v33-stable')"
      }
    }
    description = "Block SQL injection attacks"
  }
}

# Outputs
output "gke_cluster_name" {
  value = google_container_cluster.main.name
}

output "gke_cluster_endpoint" {
  value     = google_container_cluster.main.endpoint
  sensitive = true
}

output "db_connection_name" {
  value = google_sql_database_instance.main.connection_name
}

output "db_private_ip" {
  value = google_sql_database_instance.main.private_ip_address
}

output "redis_host" {
  value = google_redis_instance.main.host
}

output "redis_port" {
  value = google_redis_instance.main.port
}

output "static_ip" {
  value = google_compute_global_address.main.address
}

output "artifact_registry" {
  value = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.main.repository_id}"
}

output "service_account_email" {
  value = google_service_account.app.email
}
