//! Météo : trait [`WeatherProvider`] et Open-Meteo (sans clé, sans compte).
//!
//! Les villes se cherchent par nom (géocodage Open-Meteo) : pas de géolocalisation,
//! donc pas d'autorisation de plus. Les heures rendues sont celles du lieu
//! (`timezone=auto`).

use jiff::civil::{Date, DateTime};
use serde::{Deserialize, Serialize};

use crate::net::{self, NetError};

/// Lieu choisi par l'utilisateur, gardé dans les réglages du widget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    /// Région (`admin1`) : distingue les homonymes.
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
}

impl Place {
    /// « Lyon, Auvergne-Rhône-Alpes, France ».
    pub fn label(&self) -> String {
        [&self.name, &self.region, &self.country]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Unité des températures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

impl TempUnit {
    pub fn symbol(self) -> &'static str {
        match self {
            TempUnit::Celsius => "°C",
            TempUnit::Fahrenheit => "°F",
        }
    }

    /// Unité du vent qui va avec : km/h avec les °C, mph avec les °F.
    pub fn wind_symbol(self) -> &'static str {
        match self {
            TempUnit::Celsius => "km/h",
            TempUnit::Fahrenheit => "mph",
        }
    }
}

/// Temps qu'il fait, regroupé à partir des codes WMO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    FreezingRain,
    Snow,
    Showers,
    SnowShowers,
    Thunderstorm,
    Unknown,
}

impl Condition {
    /// D'après un code météo WMO (table d'Open-Meteo).
    pub fn from_wmo(code: u16) -> Self {
        match code {
            0 | 1 => Condition::Clear,
            2 => Condition::PartlyCloudy,
            3 => Condition::Cloudy,
            45 | 48 => Condition::Fog,
            51 | 53 | 55 => Condition::Drizzle,
            56 | 57 | 66 | 67 => Condition::FreezingRain,
            61 | 63 | 65 => Condition::Rain,
            71 | 73 | 75 | 77 => Condition::Snow,
            80..=82 => Condition::Showers,
            85 | 86 => Condition::SnowShowers,
            95 | 96 | 99 => Condition::Thunderstorm,
            _ => Condition::Unknown,
        }
    }

    /// Clé stable (traductions du widget : `widgets2.cond_<clé>`).
    pub fn key(self) -> &'static str {
        match self {
            Condition::Clear => "clear",
            Condition::PartlyCloudy => "partly_cloudy",
            Condition::Cloudy => "cloudy",
            Condition::Fog => "fog",
            Condition::Drizzle => "drizzle",
            Condition::Rain => "rain",
            Condition::FreezingRain => "freezing_rain",
            Condition::Snow => "snow",
            Condition::Showers => "showers",
            Condition::SnowShowers => "snow_showers",
            Condition::Thunderstorm => "thunderstorm",
            Condition::Unknown => "unknown",
        }
    }
}

/// Temps actuel.
#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    /// Heure du lieu.
    pub time: DateTime,
    pub temperature: f64,
    pub apparent: f64,
    /// Humidité relative, en %.
    pub humidity: Option<f64>,
    pub wind: Option<f64>,
    pub condition: Condition,
    pub is_day: bool,
}

/// Prévision d'une heure.
#[derive(Debug, Clone, PartialEq)]
pub struct Hour {
    pub time: DateTime,
    pub temperature: f64,
    pub condition: Condition,
    /// Probabilité de précipitations, en %.
    pub precipitation: Option<f64>,
    pub is_day: bool,
}

/// Prévision d'un jour.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub date: Date,
    pub min: f64,
    pub max: f64,
    pub condition: Condition,
    pub precipitation: Option<f64>,
    pub sunrise: Option<DateTime>,
    pub sunset: Option<DateTime>,
}

/// Temps actuel et prévisions d'un lieu.
#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    pub unit: TempUnit,
    pub current: Current,
    /// Les prochaines heures, à partir de l'heure en cours.
    pub hours: Vec<Hour>,
    /// Aujourd'hui puis les jours suivants.
    pub days: Vec<Day>,
}

/// Fournisseur de météo.
pub trait WeatherProvider: Send + Sync {
    /// Villes dont le nom ressemble à `query` (`lang` : `fr`, `en`… pour les noms).
    fn search(&self, query: &str, lang: &str) -> Result<Vec<Place>, NetError>;
    /// Temps actuel et prévisions sur 7 jours.
    fn forecast(&self, place: &Place, unit: TempUnit) -> Result<Forecast, NetError>;
}

/// Open-Meteo : <https://open-meteo.com>, gratuit pour un usage non commercial.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenMeteo;

const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

/// Nombre d'heures gardées après l'heure en cours.
const HOURS: usize = 24;

impl WeatherProvider for OpenMeteo {
    fn search(&self, query: &str, lang: &str) -> Result<Vec<Place>, NetError> {
        let query = query.trim();
        if query.chars().count() < 2 {
            return Ok(Vec::new());
        }
        let body = net::get_text(
            GEOCODING_URL,
            &[
                ("name", query),
                ("count", "8"),
                ("language", lang),
                ("format", "json"),
            ],
        )?;
        parse_places(&body)
    }

    fn forecast(&self, place: &Place, unit: TempUnit) -> Result<Forecast, NetError> {
        let lat = place.latitude.to_string();
        let lon = place.longitude.to_string();
        let (temp, wind) = match unit {
            TempUnit::Celsius => ("celsius", "kmh"),
            TempUnit::Fahrenheit => ("fahrenheit", "mph"),
        };
        let body = net::get_text(
            FORECAST_URL,
            &[
                ("latitude", &lat),
                ("longitude", &lon),
                (
                    "current",
                    "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day",
                ),
                (
                    "hourly",
                    "temperature_2m,weather_code,precipitation_probability,is_day",
                ),
                (
                    "daily",
                    "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset",
                ),
                ("timezone", "auto"),
                ("forecast_days", "7"),
                ("temperature_unit", temp),
                ("wind_speed_unit", wind),
            ],
        )?;
        parse_forecast(&body, unit)
    }
}

#[derive(Deserialize)]
struct GeoResponse {
    #[serde(default)]
    results: Vec<GeoResult>,
}

#[derive(Deserialize)]
struct GeoResult {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    admin1: Option<String>,
}

/// Réponse du géocodage d'Open-Meteo.
pub fn parse_places(json: &str) -> Result<Vec<Place>, NetError> {
    check_api_error(json)?;
    let r: GeoResponse =
        serde_json::from_str(json).map_err(|e| NetError::Invalid(e.to_string()))?;
    Ok(r.results
        .into_iter()
        .map(|g| Place {
            name: g.name,
            region: g.admin1.unwrap_or_default(),
            country: g.country.unwrap_or_default(),
            latitude: g.latitude,
            longitude: g.longitude,
        })
        .collect())
}

/// Open-Meteo répond `{"error": true, "reason": "…"}` (HTTP 400) aux demandes refusées.
fn check_api_error(json: &str) -> Result<(), NetError> {
    #[derive(Deserialize)]
    struct ApiError {
        #[serde(default)]
        error: bool,
        #[serde(default)]
        reason: String,
    }
    match serde_json::from_str::<ApiError>(json) {
        Ok(e) if e.error => Err(NetError::Invalid(e.reason)),
        _ => Ok(()),
    }
}

#[derive(Deserialize)]
struct ForecastResponse {
    current: CurrentJson,
    hourly: HourlyJson,
    daily: DailyJson,
}

#[derive(Deserialize)]
struct CurrentJson {
    time: String,
    temperature_2m: f64,
    apparent_temperature: Option<f64>,
    relative_humidity_2m: Option<f64>,
    weather_code: Option<u16>,
    wind_speed_10m: Option<f64>,
    is_day: Option<u8>,
}

#[derive(Deserialize)]
struct HourlyJson {
    time: Vec<String>,
    temperature_2m: Vec<Option<f64>>,
    weather_code: Vec<Option<u16>>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f64>>,
    #[serde(default)]
    is_day: Vec<Option<u8>>,
}

#[derive(Deserialize)]
struct DailyJson {
    time: Vec<String>,
    weather_code: Vec<Option<u16>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
    #[serde(default)]
    sunrise: Vec<Option<String>>,
    #[serde(default)]
    sunset: Vec<Option<String>>,
}

/// Heure locale d'Open-Meteo (`2026-11-26T13:45`).
fn parse_time(s: &str) -> Result<DateTime, NetError> {
    s.parse::<DateTime>()
        .map_err(|e| NetError::Invalid(format!("heure {s:?} : {e}")))
}

fn at<T: Copy>(v: &[Option<T>], i: usize) -> Option<T> {
    v.get(i).copied().flatten()
}

/// Réponse des prévisions d'Open-Meteo.
pub fn parse_forecast(json: &str, unit: TempUnit) -> Result<Forecast, NetError> {
    check_api_error(json)?;
    let r: ForecastResponse =
        serde_json::from_str(json).map_err(|e| NetError::Invalid(e.to_string()))?;
    let c = &r.current;
    let now = parse_time(&c.time)?;
    let current = Current {
        time: now,
        temperature: c.temperature_2m,
        apparent: c.apparent_temperature.unwrap_or(c.temperature_2m),
        humidity: c.relative_humidity_2m,
        wind: c.wind_speed_10m,
        condition: c
            .weather_code
            .map_or(Condition::Unknown, Condition::from_wmo),
        is_day: c.is_day != Some(0),
    };

    // Heure en cours : la dernière heure pleine qui ne dépasse pas l'heure actuelle.
    let this_hour = now
        .with()
        .minute(0)
        .second(0)
        .subsec_nanosecond(0)
        .build()
        .unwrap_or(now);
    let h = &r.hourly;
    let mut hours = Vec::new();
    for (i, time) in h.time.iter().enumerate() {
        let time = parse_time(time)?;
        if time < this_hour {
            continue;
        }
        let Some(temperature) = at(&h.temperature_2m, i) else {
            continue;
        };
        hours.push(Hour {
            time,
            temperature,
            condition: at(&h.weather_code, i).map_or(Condition::Unknown, Condition::from_wmo),
            precipitation: at(&h.precipitation_probability, i),
            is_day: at(&h.is_day, i) != Some(0),
        });
        if hours.len() == HOURS {
            break;
        }
    }

    let d = &r.daily;
    let mut days = Vec::new();
    for (i, date) in d.time.iter().enumerate() {
        let date = date
            .parse::<Date>()
            .map_err(|e| NetError::Invalid(format!("date {date:?} : {e}")))?;
        let (Some(min), Some(max)) = (at(&d.temperature_2m_min, i), at(&d.temperature_2m_max, i))
        else {
            continue;
        };
        let time_at = |v: &[Option<String>]| {
            v.get(i)
                .and_then(|s| s.as_deref())
                .and_then(|s| s.parse::<DateTime>().ok())
        };
        days.push(Day {
            date,
            min,
            max,
            condition: at(&d.weather_code, i).map_or(Condition::Unknown, Condition::from_wmo),
            precipitation: at(&d.precipitation_probability_max, i),
            sunrise: time_at(&d.sunrise),
            sunset: time_at(&d.sunset),
        });
    }

    Ok(Forecast {
        unit,
        current,
        hours,
        days,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::{date, datetime};

    const GEO: &str = include_str!("../tests/data/open-meteo-geocoding.json");
    const GEO_EMPTY: &str = include_str!("../tests/data/open-meteo-geocoding-empty.json");
    const FORECAST: &str = include_str!("../tests/data/open-meteo-forecast.json");
    const ERROR: &str = include_str!("../tests/data/open-meteo-error.json");

    #[test]
    fn geocoding_lists_homonyms_with_their_region() {
        let places = parse_places(GEO).unwrap();
        assert_eq!(places.len(), 5);
        assert_eq!(places[0].name, "Lyon");
        assert_eq!(places[0].label(), "Lyon, Auvergne-Rhône-Alpes, France");
        assert!((places[0].latitude - 45.749).abs() < 0.01);
        assert_eq!(places[1].country, "États-Unis");
    }

    #[test]
    fn geocoding_without_result_is_empty() {
        assert!(parse_places(GEO_EMPTY).unwrap().is_empty());
    }

    #[test]
    fn forecast_has_current_hours_and_seven_days() {
        let f = parse_forecast(FORECAST, TempUnit::Celsius).unwrap();
        assert_eq!(f.current.time, datetime(2026, 11, 26, 13, 45, 0, 0));
        assert_eq!(f.current.temperature, 8.7);
        assert_eq!(f.current.apparent, 6.2);
        assert_eq!(f.current.condition, Condition::Cloudy);
        assert!(f.current.is_day);
        // Les heures commencent à l'heure en cours (13 h), pas à minuit.
        assert_eq!(f.hours.len(), 24);
        assert_eq!(f.hours[0].time, datetime(2026, 11, 26, 13, 0, 0, 0));
        assert_eq!(f.days.len(), 7);
        assert_eq!(f.days[0].date, date(2026, 11, 26));
        assert_eq!(f.days[0].condition, Condition::Showers);
        assert_eq!((f.days[3].min, f.days[3].max), (-0.1, 7.8));
        assert_eq!(f.days[3].precipitation, Some(64.0));
        assert_eq!(f.days[0].sunset, Some(datetime(2026, 11, 26, 17, 0, 0, 0)));
    }

    #[test]
    fn api_errors_are_reported() {
        assert!(matches!(
            parse_forecast(ERROR, TempUnit::Celsius),
            Err(NetError::Invalid(reason)) if reason.contains("Latitude")
        ));
        assert!(matches!(
            parse_forecast("<html>", TempUnit::Celsius),
            Err(NetError::Invalid(_))
        ));
    }

    #[test]
    fn missing_values_do_not_break_the_forecast() {
        let json = r#"{"current":{"time":"2026-01-01T10:30","temperature_2m":1.0,"weather_code":null},
            "hourly":{"time":["2026-01-01T10:00","2026-01-01T11:00"],"temperature_2m":[null,2.0],"weather_code":[1,null]},
            "daily":{"time":["2026-01-01"],"weather_code":[71],"temperature_2m_max":[3.0],"temperature_2m_min":[-2.0]}}"#;
        let f = parse_forecast(json, TempUnit::Fahrenheit).unwrap();
        assert_eq!(f.current.condition, Condition::Unknown);
        assert_eq!(f.hours.len(), 1);
        assert_eq!(f.hours[0].condition, Condition::Unknown);
        assert_eq!(f.days[0].condition, Condition::Snow);
        assert_eq!(f.days[0].sunrise, None);
    }

    #[test]
    fn wmo_codes_are_grouped() {
        assert_eq!(Condition::from_wmo(0), Condition::Clear);
        assert_eq!(Condition::from_wmo(63), Condition::Rain);
        assert_eq!(Condition::from_wmo(95), Condition::Thunderstorm);
        assert_eq!(Condition::from_wmo(1234), Condition::Unknown);
    }
}
