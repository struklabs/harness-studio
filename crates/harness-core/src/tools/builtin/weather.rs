use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use super::{http, simple_def};
use crate::tools::{PermissionClass, ToolCtx, ToolDef, ToolError, ToolErrorCode, ToolOutput};

const MAX_RESPONSE: usize = 512 * 1024;

pub fn geocode_def() -> ToolDef {
    simple_def("geocode", "Find geographic coordinates and timezone for a place name using Open-Meteo geocoding (OpenStreetMap Nominatim as fallback). Returns structured location candidates.", PermissionClass::Restricted,
        json!({"type":"object","properties":{"query":{"type":"string","description":"City or place name."},"count":{"type":"integer","minimum":1,"maximum":10}},"required":["query"],"additionalProperties":false}))
}
pub fn weather_def() -> ToolDef {
    simple_def("weather", "Get current conditions and a short daily forecast for a place using Open-Meteo. Returns structured location, current weather, and daily forecast data.", PermissionClass::Restricted,
        json!({"type":"object","properties":{"location":{"type":"string","description":"City or place name."},"forecast_days":{"type":"integer","minimum":1,"maximum":7,"description":"Daily forecast length (default 3)."}},"required":["location"],"additionalProperties":false}))
}

pub fn geocode(args: Value, ctx: &ToolCtx) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
    Box::pin(async move {
        let query = args["query"].as_str().unwrap_or_default().trim();
        if query.is_empty() {
            return Err(ToolError::invalid("query must not be empty"));
        }
        let count = args
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 10) as usize;
        let (locations, target) = lookup(query, count, ctx).await?;
        Ok(ToolOutput {
            content: json!({"query":query,"locations":locations}).to_string(),
            network_target: Some(target),
        })
    })
}

pub fn weather(args: Value, ctx: &ToolCtx) -> BoxFuture<'_, Result<ToolOutput, ToolError>> {
    Box::pin(async move {
        let name = args["location"].as_str().unwrap_or_default().trim();
        if name.is_empty() {
            return Err(ToolError::invalid("location must not be empty"));
        }
        let days = args
            .get("forecast_days")
            .and_then(Value::as_u64)
            .unwrap_or(3)
            .clamp(1, 7);
        let (locations, geo_target) = lookup(name, 1, ctx).await?;
        let location = locations.into_iter().next().ok_or_else(|| {
            ToolError::new(
                ToolErrorCode::NotFound,
                format!("no location found for {name:?}; retry with just the place name or \"Place, Region\""),
            )
        })?;
        let lat = location["latitude"]
            .as_f64()
            .ok_or_else(|| ToolError::failed("geocoding result has no latitude"))?;
        let lon = location["longitude"]
            .as_f64()
            .ok_or_else(|| ToolError::failed("geocoding result has no longitude"))?;
        let mut url = reqwest::Url::parse("https://api.open-meteo.com/v1/forecast").unwrap();
        url.query_pairs_mut().append_pair("latitude",&lat.to_string()).append_pair("longitude",&lon.to_string())
        .append_pair("current","temperature_2m,apparent_temperature,relative_humidity_2m,precipitation,weather_code,wind_speed_10m")
        .append_pair("daily","weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum")
        .append_pair("timezone","auto").append_pair("forecast_days",&days.to_string());
        let (v, target) = get_json(url.as_str(), ctx).await?;
        let current = &v["current"];
        let daily = &v["daily"];
        let mut forecast = Vec::new();
        let dates = daily["time"].as_array().cloned().unwrap_or_default();
        for (i, date) in dates.iter().enumerate() {
            let code = at_i64(&daily["weather_code"], i);
            forecast.push(json!({"date":date,"condition":condition(code),"weather_code":code,"temperature_max_c":at_f64(&daily["temperature_2m_max"],i),"temperature_min_c":at_f64(&daily["temperature_2m_min"],i),"precipitation_mm":at_f64(&daily["precipitation_sum"],i)}));
        }
        let out = json!({"location":location,"timezone":v["timezone"],"current":{"time":current["time"],"condition":condition(current["weather_code"].as_i64()),"weather_code":current["weather_code"],"temperature_c":current["temperature_2m"],"apparent_temperature_c":current["apparent_temperature"],"relative_humidity_percent":current["relative_humidity_2m"],"precipitation_mm":current["precipitation"],"wind_speed_kmh":current["wind_speed_10m"]},"daily":forecast});
        Ok(ToolOutput {
            content: out.to_string(),
            network_target: Some(format!("{geo_target}, {target}")),
        })
    })
}

/// Open-Meteo first (it also returns the timezone); its name search misses multi-word
/// queries ("İzmir Narlıdere") and ASCII spellings ("Narlidere"), so an empty result falls
/// back to OpenStreetMap Nominatim's free-text search.
async fn lookup(
    query: &str,
    count: usize,
    ctx: &ToolCtx,
) -> Result<(Vec<Value>, String), ToolError> {
    let mut url = reqwest::Url::parse("https://geocoding-api.open-meteo.com/v1/search").unwrap();
    url.query_pairs_mut()
        .append_pair("name", query)
        .append_pair("count", &count.to_string())
        .append_pair("language", "en")
        .append_pair("format", "json");
    let (v, target) = get_json(url.as_str(), ctx).await?;
    let locations: Vec<Value> = v["results"].as_array().into_iter().flatten().map(|r|json!({"name":r["name"],"country":r["country"],"admin1":r["admin1"],"admin2":r["admin2"],"latitude":r["latitude"],"longitude":r["longitude"],"timezone":r["timezone"]})).collect();
    if !locations.is_empty() {
        return Ok((locations, target));
    }
    let (fallback, osm_target) = nominatim(query, count, ctx).await?;
    Ok((fallback, format!("{target}, {osm_target}")))
}

async fn nominatim(
    query: &str,
    count: usize,
    ctx: &ToolCtx,
) -> Result<(Vec<Value>, String), ToolError> {
    let mut url = reqwest::Url::parse("https://nominatim.openstreetmap.org/search").unwrap();
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("format", "jsonv2")
        .append_pair("limit", &count.to_string())
        .append_pair("addressdetails", "1")
        .append_pair("accept-language", "en");
    let (v, target) = get_json(url.as_str(), ctx).await?;
    let locations = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let lat = r["lat"].as_str()?.parse::<f64>().ok()?;
            let lon = r["lon"].as_str()?.parse::<f64>().ok()?;
            let a = &r["address"];
            let admin1 = [&a["province"], &a["state"]]
                .into_iter()
                .find(|x| x.is_string())
                .cloned()
                .unwrap_or(Value::Null);
            Some(json!({"name":r["name"],"country":a["country"],"admin1":admin1,"admin2":a["county"],"latitude":lat,"longitude":lon,"timezone":Value::Null}))
        })
        .collect();
    Ok((locations, target))
}
/// GET + JSON with one retry: the free Open-Meteo/Nominatim services
/// occasionally answer 5xx or time out for a moment (S37).
async fn get_json(url: &str, ctx: &ToolCtx) -> Result<(Value, String), ToolError> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let result = match http::get(url, ctx).await {
            Ok((response, final_url)) => {
                let target = http::target(&final_url);
                read_json(response, &target).await.map(|v| (v, target))
            }
            Err(e) => Err(e),
        };
        match result {
            Err(e) if attempt == 1 && is_transient(&e) => {
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            }
            other => return other,
        }
    }
}
fn is_transient(e: &ToolError) -> bool {
    match e.code {
        ToolErrorCode::Timeout => true,
        ToolErrorCode::Network => e.message.contains("HTTP 5") || !e.message.contains("HTTP "),
        _ => false,
    }
}
async fn read_json(response: reqwest::Response, target: &str) -> Result<Value, ToolError> {
    if !response.status().is_success() {
        return Err(ToolError::new(
            ToolErrorCode::Network,
            format!(
                "geocoding/weather service returned HTTP {}",
                response.status()
            ),
        )
        .network_target(target));
    }
    let b = http::bytes_limited(response, MAX_RESPONSE, target).await?;
    serde_json::from_slice(&b).map_err(|e| {
        ToolError::new(
            ToolErrorCode::Network,
            format!("geocoding/weather service returned invalid JSON: {e}"),
        )
        .network_target(target)
    })
}
fn at_f64(v: &Value, i: usize) -> Option<f64> {
    v.as_array()?.get(i)?.as_f64()
}
fn at_i64(v: &Value, i: usize) -> Option<i64> {
    v.as_array()?.get(i)?.as_i64()
}
fn condition(code: Option<i64>) -> &'static str {
    match code {
        Some(0) => "clear sky",
        Some(1 | 2) => "partly cloudy",
        Some(3) => "overcast",
        Some(45 | 48) => "fog",
        Some(51 | 53 | 55 | 56 | 57) => "drizzle",
        Some(61 | 63 | 65 | 66 | 67) => "rain",
        Some(71 | 73 | 75 | 77) => "snow",
        Some(80..=82) => "rain showers",
        Some(85 | 86) => "snow showers",
        Some(95 | 96 | 99) => "thunderstorm",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_transient_errors() {
        let net = |m: &str| ToolError::new(ToolErrorCode::Network, m.to_string());
        assert!(is_transient(&net(
            "geocoding/weather service returned HTTP 503 Service Unavailable"
        )));
        assert!(is_transient(&net("connection reset")));
        assert!(is_transient(&ToolError::new(
            ToolErrorCode::Timeout,
            "timed out".to_string()
        )));
        assert!(!is_transient(&net(
            "geocoding/weather service returned HTTP 404 Not Found"
        )));
        assert!(!is_transient(&ToolError::invalid("bad")));
    }
    #[tokio::test]
    #[ignore = "requires public Open-Meteo API"]
    async fn real_open_meteo_weather() {
        let ctx = ToolCtx::new(None);
        let out = weather(json!({"location":"Istanbul","forecast_days":2}), &ctx)
            .await
            .unwrap();
        let v: Value = serde_json::from_str(&out.content).unwrap();
        println!(
            "target={} location={} temp={} daily={}",
            out.network_target.unwrap(),
            v["location"]["name"],
            v["current"]["temperature_c"],
            v["daily"].as_array().unwrap().len()
        );
        assert_eq!(v["daily"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    #[ignore = "requires public Open-Meteo and Nominatim APIs"]
    async fn real_lookup_falls_back_for_multi_word_and_ascii_names() {
        let ctx = ToolCtx::new(None);
        for q in ["İzmir Narlıdere", "Narlidere"] {
            let (locs, target) = lookup(q, 3, &ctx).await.unwrap();
            println!("{q}: target={target} first={}", locs[0]);
            assert!(locs[0]["admin1"].as_str().unwrap().contains("zmir"));
        }
    }
}
