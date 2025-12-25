# ATOM Intents Demo - Terraform Variables

variable "project_id" {
  description = "The GCP project ID"
  type        = string
}

variable "region" {
  description = "The GCP region for resources"
  type        = string
  default     = "us-central1"
}

variable "zone" {
  description = "The GCP zone for zonal resources"
  type        = string
  default     = "us-central1-a"
}

variable "environment" {
  description = "The environment (dev, staging, prod)"
  type        = string
  default     = "dev"

  validation {
    condition     = contains(["dev", "staging", "prod"], var.environment)
    error_message = "Environment must be dev, staging, or prod."
  }
}

variable "domain" {
  description = "The domain name for the demo (optional, enables HTTPS)"
  type        = string
  default     = ""
}

variable "skip_select_image" {
  description = "Container image for Skip Select Simulator"
  type        = string
  default     = ""
}

variable "web_ui_image" {
  description = "Container image for Web UI"
  type        = string
  default     = ""
}
