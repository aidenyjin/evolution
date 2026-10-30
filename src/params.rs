//! Every tunable constant of the simulated universe lives here. The registry
//! is sent to the browser, which builds the settings panel from it, so adding
//! a parameter here automatically makes it adjustable in the UI.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct ParamDesc {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub unit: &'static str,
    pub default: f32,
    pub min: f32,
    pub max: f32,
    /// Changing this only takes effect when a new world is created.
    pub reset: bool,
    pub desc: &'static str,
}

macro_rules! params {
    ($( $group:literal => { $( $field:ident : $label:literal, $unit:literal, $default:expr, $min:expr, $max:expr, $reset:expr, $desc:literal; )* } )*) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Params { $($( pub $field: f32, )*)* }

        impl Default for Params {
            fn default() -> Self { Self { $($( $field: $default, )*)* } }
        }

        impl Params {
            #[allow(dead_code)]
            pub fn get(&self, key: &str) -> Option<f32> {
                match key { $($( stringify!($field) => Some(self.$field), )*)* _ => None }
            }
            pub fn set(&mut self, key: &str, v: f32) -> bool {
                if !v.is_finite() { return false; }
                match key { $($( stringify!($field) => { self.$field = v.clamp($min, $max); true } )*)* _ => false }
            }
            pub fn descriptors() -> Vec<ParamDesc> {
                vec![ $($( ParamDesc { key: stringify!($field), label: $label, group: $group, unit: $unit,
                    default: $default, min: $min, max: $max, reset: $reset, desc: $desc }, )*)* ]
            }
            pub fn values(&self) -> Vec<(&'static str, f32)> {
                vec![ $($( (stringify!($field), self.$field), )*)* ]
            }
        }
    };
}

params! {
    "World" => {
        world_width: "World width", "µm", 960.0, 300.0, 2400.0, true,
            "Width of the simulated strip of seafloor, from open deep ocean (left) to the beach (right).";
        world_height: "World height", "µm", 600.0, 200.0, 1600.0, true,
            "Height of the simulated strip.";
        grid_res: "Chemistry grid spacing", "µm", 4.0, 2.0, 12.0, true,
            "Resolution of the dissolved-chemical grid. Smaller is more detailed and slower.";
        deep_end: "Deep ocean ends at", "fraction", 0.40, 0.05, 0.9, true,
            "Fraction of the width (from the left) occupied by deep ocean.";
        shallow_end: "Shallow ocean ends at", "fraction", 0.80, 0.1, 0.97, true,
            "Fraction of the width where shallow water gives way to the beach.";
        zone_blend: "Zone transition width", "fraction", 0.04, 0.005, 0.2, true,
            "How gradually one zone blends into the next.";
        vent_count: "Hydrothermal vents", "", 7.0, 0.0, 16.0, true,
            "Number of vents scattered through the deep ocean.";
        initial_cells: "Starting LUCA cells", "", 60.0, 1.0, 1000.0, true,
            "How many copies of the ancestral cell are placed around the vents at the start.";
    }
    "Light" => {
        light_deep: "Light in deep ocean", "relative", 0.06, 0.0, 1.0, false,
            "Sunlight reaching the deep zone (1 = full sun).";
        light_shallow: "Light in shallow ocean", "relative", 0.75, 0.0, 1.0, false,
            "Sunlight reaching the shallow zone.";
        light_beach: "Light on the beach", "relative", 1.0, 0.0, 1.0, false,
            "Sunlight reaching the wet shoreline.";
        day_night: "Day/night cycle", "on/off", 0.0, 0.0, 1.0, false,
            "1 = sunlight rises and sets, 0 = constant light.";
        day_length: "Day length", "s", 600.0, 30.0, 7200.0, false,
            "Length of one full day/night cycle in simulated seconds.";
    }
    "Temperature" => {
        temp_deep: "Deep water temperature", "°C", 6.0, -2.0, 60.0, false,
            "Ambient temperature of the deep ocean away from vents.";
        temp_shallow: "Shallow water temperature", "°C", 20.0, -2.0, 60.0, false,
            "Ambient temperature of the shallow ocean.";
        temp_beach: "Beach temperature", "°C", 27.0, -2.0, 70.0, false,
            "Ambient temperature of the wet shoreline.";
        vent_temp: "Vent core temperature", "°C", 75.0, 10.0, 120.0, false,
            "Water temperature right at a vent opening.";
        vent_heat_radius: "Vent heat radius", "µm", 30.0, 5.0, 150.0, true,
            "How far vent heat reaches before fading into the cold deep.";
        q10: "Q10 temperature coefficient", "×", 2.0, 1.0, 4.0, false,
            "How much faster chemistry runs per 10 °C warmer (Arrhenius behaviour; ~2 for most enzymes).";
        thermal_tolerance: "Enzyme thermal tolerance", "°C", 14.0, 3.0, 40.0, false,
            "Width of each protein's temperature-activity curve around its evolved optimum.";
        heat_damage: "Heat damage rate", "/s", 0.01, 0.0, 0.2, false,
            "Protein denaturation damage per second per 10 °C above a cell's tolerance.";
    }
    "Vents" => {
        vent_h2: "Vent H₂ output", "units/s", 450.0, 0.0, 2000.0, false,
            "Hydrogen released by each vent.";
        vent_h2s: "Vent H₂S output", "units/s", 60.0, 0.0, 1000.0, false,
            "Hydrogen sulfide released by each vent.";
        vent_minerals: "Vent mineral output", "units/s", 6.0, 0.0, 100.0, false,
            "Ammonium released by each vent (phosphate is released at 1/8 of this).";
        vent_radius: "Vent opening radius", "µm", 10.0, 2.0, 60.0, true,
            "Size of the area each vent releases chemicals into.";
        vent_variability: "Vent pulsing", "fraction", 0.35, 0.0, 1.0, false,
            "How strongly vent output fluctuates over time.";
        vent_period: "Vent pulse period", "s", 400.0, 20.0, 5000.0, false,
            "Typical time scale of vent output fluctuations.";
    }
    "Water & waves" => {
        current_strength: "Ocean current speed", "µm/s", 2.5, 0.0, 30.0, false,
            "Typical speed of slow swirling currents in shallow water.";
        current_deep_factor: "Deep current factor", "×", 0.25, 0.0, 2.0, false,
            "Deep water currents relative to shallow water (the deep seafloor is calm).";
        current_scale: "Current eddy size", "µm", 320.0, 40.0, 2000.0, false,
            "Typical size of current eddies.";
        current_drift: "Current change rate", "/s", 0.004, 0.0, 0.1, false,
            "How quickly the current pattern evolves.";
        wave_strength: "Wave surge speed", "µm/s", 16.0, 0.0, 120.0, false,
            "Peak speed of the back-and-forth wave surge on the beach.";
        wave_period: "Wave period", "s", 7.0, 1.0, 60.0, false,
            "Time between waves.";
        flow_coupling: "Cell drift with water", "×", 1.0, 0.0, 1.0, false,
            "How strongly cells are carried by moving water.";
    }
    "Chemistry" => {
        diff_gas: "Diffusion: dissolved gases", "µm²/s", 220.0, 1.0, 3000.0, false,
            "Diffusion coefficient of CO₂, H₂, H₂S, O₂. Real values are ~1000–2000 µm²/s; the default is lower because the simulated population is far sparser than a real microbial mat.";
        diff_organic: "Diffusion: small organics", "µm²/s", 120.0, 1.0, 3000.0, false,
            "Diffusion coefficient of sugar and acetate (real: ~500–1000).";
        diff_ion: "Diffusion: ions", "µm²/s", 160.0, 1.0, 3000.0, false,
            "Diffusion coefficient of sulfate, ammonium and phosphate (real: ~1000–2000).";
        diff_dom: "Diffusion: dead-cell organics", "µm²/s", 20.0, 0.1, 1000.0, false,
            "Diffusion coefficient of dissolved organic matter released by dead cells (large molecules).";
        gas_exchange: "Gas exchange rate", "/s", 0.02, 0.0, 1.0, false,
            "How fast dissolved gases equilibrate with the atmosphere in shallow water.";
        gas_exchange_deep: "Deep gas exchange factor", "×", 0.08, 0.0, 2.0, false,
            "Gas exchange in the deep relative to shallow water.";
        gas_exchange_beach: "Beach gas exchange factor", "×", 1.5, 0.0, 5.0, false,
            "Gas exchange on the beach relative to shallow water (waves mix air in).";
        atm_co2: "Atmospheric CO₂ level", "units/µm²", 3.0, 0.0, 20.0, false,
            "Dissolved CO₂ concentration the water tends toward. Early Earth had a lot.";
        atm_o2: "Atmospheric O₂ level", "units/µm²", 0.0, 0.0, 10.0, false,
            "Dissolved O₂ the water tends toward. 0 = early Earth (no free oxygen).";
        mineral_supply: "Mineral resupply rate", "/s", 0.004, 0.0, 0.2, false,
            "How fast ammonium, phosphate and sulfate are replenished toward each zone's background level.";
        nh4_deep: "Ammonium: deep", "units/µm²", 0.04, 0.0, 5.0, false, "Background ammonium (nitrogen source) in deep water.";
        nh4_shallow: "Ammonium: shallow", "units/µm²", 0.12, 0.0, 5.0, false, "Background ammonium in shallow water.";
        nh4_beach: "Ammonium: beach", "units/µm²", 0.45, 0.0, 5.0, false, "Background ammonium on the beach (runoff from land).";
        po4_deep: "Phosphate: deep", "units/µm²", 0.008, 0.0, 2.0, false, "Background phosphate in deep water.";
        po4_shallow: "Phosphate: shallow", "units/µm²", 0.025, 0.0, 2.0, false, "Background phosphate in shallow water.";
        po4_beach: "Phosphate: beach", "units/µm²", 0.10, 0.0, 2.0, false, "Background phosphate on the beach (weathered rock).";
        so4_background: "Sulfate background", "units/µm²", 0.05, 0.0, 5.0, false, "Background sulfate. Early oceans had little.";
        sulfide_oxidation: "Abiotic sulfide oxidation", "/(units·s)", 0.3, 0.0, 10.0, false,
            "Rate at which H₂S reacts with O₂ spontaneously, without any cell involved.";
        dom_hydrolysis: "Dead matter breakdown", "/s", 0.006, 0.0, 1.0, false,
            "Rate at which dead-cell organic matter spontaneously breaks down into sugar.";
    }
    "Thermodynamics" => {
        atp_energy: "ATP energy", "kJ/mol", 50.0, 20.0, 100.0, false,
            "Free energy stored per ATP under cellular conditions.";
        efficiency: "Energy capture efficiency", "fraction", 0.6, 0.1, 0.95, false,
            "Fraction of a reaction's free energy a cell can capture as ATP (and inverse cost for uphill reactions). Must stay below 1 or perpetual motion becomes possible.";
        g_co2: "ΔGf CO₂", "kJ/mol", -394.4, -600.0, 0.0, false, "Formation free energy of CO₂ (all reaction energies are derived from these).";
        g_h2: "ΔGf H₂", "kJ/mol", 0.0, -100.0, 100.0, false, "Formation free energy of H₂.";
        g_h2s: "ΔGf H₂S", "kJ/mol", -27.9, -200.0, 100.0, false, "Formation free energy of H₂S.";
        g_o2: "ΔGf O₂", "kJ/mol", 0.0, -100.0, 100.0, false, "Formation free energy of O₂.";
        g_sugar: "ΔGf sugar (per C)", "kJ/mol", -152.9, -400.0, 0.0, false, "Formation free energy of one carbon's worth of sugar (glucose/6).";
        g_acetate: "ΔGf acetate", "kJ/mol", -369.4, -700.0, 0.0, false, "Formation free energy of acetate.";
        g_so4: "ΔGf sulfate", "kJ/mol", -744.6, -1200.0, 0.0, false, "Formation free energy of sulfate.";
        min_fix_cost: "Min. carbon fixation cost", "ATP", 0.5, 0.0, 5.0, false,
            "Minimum ATP spent per carbon fixed, for the enzymatic machinery, even if the chemistry is nearly free.";
        photo_atp: "Photosynthesis ATP bonus", "ATP", 0.3, 0.0, 3.0, false,
            "ATP made per photosynthetic turnover (cyclic photophosphorylation).";
        pump_atp: "Light pump ATP yield", "ATP", 0.4, 0.0, 3.0, false,
            "ATP made per turnover of a light-driven proton pump.";
        light_half_sat: "Light half-saturation", "relative", 0.3, 0.01, 2.0, false,
            "Light intensity at which photosynthesis runs at half speed.";
    }
    "Enzyme speeds" => {
        bio_pace: "Biological pace", "×", 1.0, 0.05, 20.0, false,
            "Speeds up or slows down all of biology (reactions, growth, damage, replication) together, keeping their ratios.";
        k_acetogenesis: "Acetogenesis", "/s", 0.5, 0.0, 10.0, false, "4H₂ + 2CO₂ → acetate (chemosynthesis). Max turnovers per enzyme unit per second.";
        k_h2fix: "H₂ carbon fixation", "/s", 0.35, 0.0, 10.0, false, "2H₂ + CO₂ → sugar.";
        k_acet_assim: "Acetate assimilation", "/s", 0.3, 0.0, 10.0, false, "Acetate → 2 sugar (costs ATP).";
        k_ferment: "Fermentation", "/s", 0.6, 0.0, 10.0, false, "3 sugar → acetate + CO₂ + 2H₂.";
        k_resp: "Aerobic respiration", "/s", 0.4, 0.0, 10.0, false, "Sugar + O₂ → CO₂.";
        k_acet_ox: "Acetate oxidation", "/s", 0.25, 0.0, 10.0, false, "Acetate + 2O₂ → 2CO₂.";
        k_sulf_red_ac: "Sulfate reduction (acetate)", "/s", 0.4, 0.0, 10.0, false, "Acetate + sulfate → H₂S + 2CO₂.";
        k_sulf_red_h2: "Sulfate reduction (H₂)", "/s", 0.4, 0.0, 10.0, false, "4H₂ + sulfate → H₂S.";
        k_sulfide_ox: "Sulfide oxidation", "/s", 0.25, 0.0, 10.0, false, "H₂S + 2O₂ → sulfate.";
        k_h2_ox: "Hydrogen oxidation", "/s", 0.3, 0.0, 10.0, false, "2H₂ + O₂ → water.";
        k_anoxy_photo: "Anoxygenic photosynthesis", "/s", 0.35, 0.0, 10.0, false, "H₂S + 2CO₂ + light → 2 sugar + sulfate.";
        k_oxy_photo: "Oxygenic photosynthesis", "/s", 0.3, 0.0, 10.0, false, "CO₂ + light → sugar + O₂.";
        k_proton_pump: "Light-driven proton pump", "/s", 0.6, 0.0, 10.0, false, "Light → ATP, no carbon fixed.";
        k_ros_detox: "ROS detoxification", "/s", 4.0, 0.0, 50.0, false, "Reactive oxygen → harmless (catalase/SOD-like).";
        km_gas: "Half-saturation: gases", "units/µm²", 0.3, 0.001, 10.0, false, "Internal gas concentration at which enzymes run at half speed.";
        km_organic: "Half-saturation: organics", "units/µm²", 0.2, 0.001, 10.0, false, "Same, for sugar and acetate.";
        km_ion: "Half-saturation: ions", "units/µm²", 0.02, 0.0005, 5.0, false, "Same, for ammonium, phosphate and sulfate.";
        km_atp: "Half-saturation: ATP", "per µm²", 0.3, 0.001, 10.0, false, "ATP concentration at which ATP-consuming processes run at half speed.";
    }
    "Membrane & transport" => {
        perm_gas: "Gas permeability", "µm/s", 3.0, 0.0, 50.0, false,
            "Small gases (CO₂, H₂, H₂S, O₂) cross lipid membranes on their own.";
        perm_acetate: "Acetate leakiness", "µm/s", 0.06, 0.0, 5.0, false,
            "Acetic acid slowly leaks through membranes without a transporter.";
        transporter_perm: "Transporter strength", "µm/s", 6.0, 0.0, 100.0, false,
            "Permeability added per unit of transporter protein (proteome share).";
        transport_accumulation: "Active transport concentration", "×", 30.0, 1.0, 1000.0, false,
            "How many times above the outside level an energised transporter can concentrate a nutrient inside the cell.";
        transport_atp: "Active transport cost", "ATP/unit", 0.1, 0.0, 5.0, false,
            "ATP spent per unit of nutrient pumped in by a transporter.";
    }
    "Growth & division" => {
        growth_rate: "Max growth rate", "/s", 0.08, 0.0, 2.0, false,
            "Biomass built per second per unit biomass when ribosomes are 100% of the proteome and fully supplied.";
        growth_atp: "ATP per biomass", "ATP", 1.5, 0.0, 20.0, false, "Energy needed to build one unit of biomass.";
        biomass_n: "Nitrogen per biomass", "units", 0.15, 0.0, 2.0, false, "Ammonium needed per unit biomass (Redfield-like ratio).";
        biomass_p: "Phosphorus per biomass", "units", 0.012, 0.0, 1.0, false, "Phosphate needed per unit biomass.";
        maintenance: "Maintenance energy", "ATP/µm²/s", 0.003, 0.0, 0.1, false, "ATP every unit of biomass burns just to stay alive.";
        atp_capacity: "ATP storage capacity", "ATP/µm²", 4.0, 0.1, 50.0, false, "Maximum ATP a cell can hold per unit biomass.";
        protein_turnover: "Protein turnover", "/s", 0.15, 0.001, 5.0, false, "How fast protein levels adjust toward what the genes dictate.";
        replication_speed: "DNA replication speed", "codons/s", 30.0, 1.0, 1000.0, false, "How fast the genome is copied. Longer genomes take longer.";
        replication_atp: "Replication ATP cost", "ATP/codon", 0.03, 0.0, 1.0, false, "Energy to copy one codon of DNA.";
        replication_np: "Replication N & P cost", "units/codon", 0.002, 0.0, 0.1, false, "Ammonium and phosphate used per codon copied.";
        division_duration: "Division duration", "s", 2.5, 0.2, 30.0, false, "Time for the membrane to pinch in two.";
        division_noise: "Division asymmetry", "fraction", 0.04, 0.0, 0.3, false, "Random unevenness when contents are split between daughters.";
        min_size: "Starvation size limit", "fraction", 0.25, 0.05, 0.9, false, "A cell that shrinks below this fraction of its division size dies.";
        max_population: "Population safety cap", "cells", 30000.0, 100.0, 200000.0, false, "Division pauses above this many cells, to protect your computer.";
    }
    "Damage & death" => {
        aging_rate: "Aging damage", "/s", 0.0015, 0.0, 0.1, false, "Baseline molecular damage per second.";
        ros_from_o2: "ROS from oxygen", "/s", 0.15, 0.0, 5.0, false, "Reactive oxygen species formed per unit internal O₂.";
        ros_leak: "Respiratory ROS leak", "fraction", 0.03, 0.0, 0.5, false, "Fraction of O₂ used in respiration that leaks out as reactive oxygen.";
        ros_damage: "ROS damage", "/s", 0.6, 0.0, 20.0, false, "Damage per second per unit internal ROS concentration.";
        ros_decay: "ROS spontaneous decay", "/s", 0.5, 0.0, 20.0, false, "Reactive oxygen decays on its own at this rate.";
        repair_rate: "Repair rate", "/s", 0.01, 0.0, 1.0, false, "Damage a cell can repair per second if it has energy.";
        repair_atp: "Repair cost", "ATP/µm²", 2.0, 0.0, 50.0, false, "ATP per unit biomass to repair one full unit of damage.";
        starvation_damage: "Starvation damage", "/s", 0.04, 0.0, 1.0, false, "Damage per second while out of energy.";
        autophagy_yield: "Self-digestion yield", "ATP/µm²", 1.0, 0.0, 10.0, false, "ATP recovered by digesting own biomass when starving.";
    }
    "Mutation" => {
        mut_point: "Point mutation rate", "/codon", 0.002, 0.0, 0.1, false, "Chance per codon per replication that one base changes.";
        mut_indel: "Insertion/deletion rate", "/codon", 0.0004, 0.0, 0.05, false, "Chance per codon of inserting or deleting a codon.";
        mut_dup: "Gene duplication", "/genome", 0.03, 0.0, 1.0, false, "Chance per replication that a gene is duplicated.";
        mut_del: "Gene loss", "/genome", 0.02, 0.0, 1.0, false, "Chance per replication that a gene is deleted.";
        mut_new_gene: "New gene from noise", "/genome", 0.03, 0.0, 1.0, false, "Chance that a random stretch of DNA becomes a new expressed gene.";
        mut_promoter: "Promoter mutation", "/gene", 0.03, 0.0, 1.0, false, "Chance per gene that its expression strength changes.";
        mut_promoter_size: "Promoter mutation size", "log", 0.25, 0.0, 2.0, false, "Typical size of expression-strength changes.";
        mut_operator: "Operator mutation", "/gene", 0.02, 0.0, 1.0, false, "Chance per gene that its regulatory binding site changes.";
        mut_control: "Machinery mutation", "/locus", 0.05, 0.0, 1.0, false, "Chance that a universal-machinery setting (ribosome level, division size) changes.";
        mut_control_size: "Machinery mutation size", "log", 0.08, 0.0, 1.0, false, "Typical size of machinery setting changes.";
        max_genes: "Max genes per genome", "", 64.0, 4.0, 256.0, false, "Performance limit on genome size.";
    }
    "Physics" => {
        viscosity: "Water viscosity", "× water", 1.0, 0.1, 20.0, false, "Thicker water slows swimming and Brownian motion.";
        brownian: "Brownian motion", "× real", 1.0, 0.0, 50.0, false, "Thermal jiggling of cells (Stokes–Einstein). 1 = physically accurate.";
        collision_stiffness: "Collision stiffness", "", 0.35, 0.01, 1.0, false, "How firmly cells push each other apart.";
        membrane_tension: "Membrane tension", "", 0.3, 0.0, 1.0, false, "How strongly membranes resist wrinkling.";
        membrane_relax: "Membrane elasticity", "", 0.12, 0.0, 1.0, false, "How quickly squashed membranes spring back.";
        motor_speed: "Max swimming speed", "µm/s", 25.0, 0.0, 200.0, false, "Top swimming speed of a cell full of motor proteins.";
        motor_cost: "Swimming energy cost", "ATP·s/µm²", 0.0005, 0.0, 0.05, false, "ATP per unit biomass per (µm/s)² of swimming.";
        tumble_rate: "Tumble rate", "/s", 0.4, 0.0, 10.0, false, "How often swimming cells randomly change direction.";
    }
}
