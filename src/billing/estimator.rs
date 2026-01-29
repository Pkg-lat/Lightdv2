//! Cost estimation for containers and volumes
//! 
//! Provides estimates for different time periods without actual usage data

use super::tracker::BillingRates;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    pub memory_gb: f64,
    pub cpu_vcpus: f64,
    pub storage_gb: f64,
    pub egress_gb_per_month: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEstimate {
    pub hourly: f64,
    pub daily: f64,
    pub monthly: f64,
    pub breakdown: CostBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdown {
    pub memory: PeriodCosts,
    pub cpu: PeriodCosts,
    pub storage: PeriodCosts,
    pub egress: PeriodCosts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodCosts {
    pub hourly: f64,
    pub daily: f64,
    pub monthly: f64,
}

pub struct CostEstimator {
    rates: BillingRates,
}

impl CostEstimator {
    pub fn new(rates: BillingRates) -> Self {
        Self { rates }
    }
    
    /// Estimate costs for a given resource configuration
    pub fn estimate(&self, config: &ResourceConfig) -> CostEstimate {
        // Memory costs
        let memory_hourly = config.memory_gb * self.rates.memory_per_gb_hour;
        let memory_daily = memory_hourly * 24.0;
        let memory_monthly = memory_daily * 30.0;
        
        // CPU costs
        let cpu_hourly = config.cpu_vcpus * self.rates.cpu_per_vcpu_hour;
        let cpu_daily = cpu_hourly * 24.0;
        let cpu_monthly = cpu_daily * 30.0;
        
        // Storage costs
        let storage_hourly = config.storage_gb * self.rates.storage_per_gb_hour;
        let storage_daily = storage_hourly * 24.0;
        let storage_monthly = storage_daily * 30.0;
        
        // Egress costs (monthly estimate distributed)
        let egress_monthly = config.egress_gb_per_month * self.rates.egress_per_gb;
        let egress_daily = egress_monthly / 30.0;
        let egress_hourly = egress_daily / 24.0;
        
        // Total costs
        let hourly = memory_hourly + cpu_hourly + storage_hourly + egress_hourly;
        let daily = memory_daily + cpu_daily + storage_daily + egress_daily;
        let monthly = memory_monthly + cpu_monthly + storage_monthly + egress_monthly;
        
        CostEstimate {
            hourly,
            daily,
            monthly,
            breakdown: CostBreakdown {
                memory: PeriodCosts {
                    hourly: memory_hourly,
                    daily: memory_daily,
                    monthly: memory_monthly,
                },
                cpu: PeriodCosts {
                    hourly: cpu_hourly,
                    daily: cpu_daily,
                    monthly: cpu_monthly,
                },
                storage: PeriodCosts {
                    hourly: storage_hourly,
                    daily: storage_daily,
                    monthly: storage_monthly,
                },
                egress: PeriodCosts {
                    hourly: egress_hourly,
                    daily: egress_daily,
                    monthly: egress_monthly,
                },
            },
        }
    }
    
    /// Estimate costs for a container configuration
    #[allow(dead_code)]
    pub fn estimate_container(
        &self,
        memory_mb: u64,
        cpu_cores: f64,
        storage_gb: f64,
        egress_gb_per_month: f64,
    ) -> CostEstimate {
        let config = ResourceConfig {
            memory_gb: memory_mb as f64 / 1024.0,
            cpu_vcpus: cpu_cores,
            storage_gb,
            egress_gb_per_month,
        };
        
        self.estimate(&config)
    }
    
    /// Estimate costs for a volume
    pub fn estimate_volume(&self, size_gb: f64) -> CostEstimate {
        let config = ResourceConfig {
            memory_gb: 0.0,
            cpu_vcpus: 0.0,
            storage_gb: size_gb,
            egress_gb_per_month: 0.0,
        };
        
        self.estimate(&config)
    }
    
    /// Compare two configurations
    #[allow(dead_code)]
    pub fn compare(&self, config_a: &ResourceConfig, config_b: &ResourceConfig) -> (CostEstimate, CostEstimate, f64) {
        let estimate_a = self.estimate(config_a);
        let estimate_b = self.estimate(config_b);
        
        let difference_monthly = estimate_b.monthly - estimate_a.monthly;
        let percentage_change = if estimate_a.monthly > 0.0 {
            (difference_monthly / estimate_a.monthly) * 100.0
        } else {
            0.0
        };
        
        (estimate_a, estimate_b, percentage_change)
    }
}
