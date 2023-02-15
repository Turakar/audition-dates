use anyhow::anyhow;
use anyhow::Result;
use chrono::DateTime;
use chrono::Duration;
use chrono::Local;
use chrono::NaiveDateTime;
use rocket_db_pools::Connection;
use serde::Deserialize;
use serde::Serialize;

use crate::util::datetime_to_day;
use crate::Config;
use crate::Database;

#[derive(Serialize, Deserialize)]
pub struct Voice {
    pub value: String,
    pub display_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DateType {
    pub value: String,
    pub display_name: Option<String>,
}

impl DateType {
    pub async fn get_by_value(
        db: &mut Connection<Database>,
        value: &str,
        lang: &str,
    ) -> Result<Self> {
        let display_name = sqlx::query_scalar!(
            r#"select display_name
            from date_types_translations
            where date_type = $1 and lang = $2"#,
            &value,
            &lang
        )
        .fetch_one(&mut **db)
        .await?;
        Ok(Self {
            value: String::from(value),
            display_name: Some(display_name),
        })
    }

    pub async fn get_variants(db: &mut Connection<Database>, lang: &str) -> Result<Vec<Self>> {
        Ok(sqlx::query!(
            r#"select id, display_name
            from date_types
            join date_types_translations on date_types.id = date_types_translations.date_type
            where lang = $1"#,
            &lang
        )
        .fetch_all(&mut **db)
        .await?
        .into_iter()
        .map(|record| Self {
            value: record.id,
            display_name: Some(record.display_name),
        })
        .collect())
    }

    pub async fn get_voices(
        &self,
        db: &mut Connection<Database>,
        lang: &str,
        position: &str,
    ) -> Result<Vec<Voice>> {
        Ok(sqlx::query!(
            "select value, display_name \
        from voices \
        join voices_translations on voices.id = voices_translations.voice \
        where lang = $1 \
        and position::text = $2 \
        and date_type = $3",
            &lang,
            &position,
            &self.value,
        )
        .fetch_all(&mut **db)
        .await?
        .into_iter()
        .map(|record| Voice {
            value: record.value,
            display_name: Some(record.display_name),
        })
        .collect())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Date {
    pub id: i32,
    pub from_date: DateTime<Local>,
    pub to_date: DateTime<Local>,
    pub room_number: String,
    pub date_type: DateType,
}

impl Date {
    pub async fn get_available_dates(
        db: &mut Connection<Database>,
        date_type: &str,
        config: &Config,
        lang: Option<&str>,
    ) -> Result<Vec<Date>> {
        if let Some(deadline) = config.application_deadlines.get(date_type) {
            let deadline = NaiveDateTime::parse_from_str(deadline, crate::BROWSER_DATETIME_FORMAT)?;
            let now = Local::now().naive_local();
            if now >= deadline {
                return Ok(Vec::new());
            }
        }

        let mut dates: Vec<Date> = match lang {
            Some(lang) => sqlx::query!(
                "select dates.id as id, from_date, to_date, room_number, dates.date_type, display_name \
                    from dates \
                    join rooms on rooms.id = dates.room_id \
                    left join bookings on dates.id = bookings.date_id \
                    join date_types_translations on date_types_translations.date_type = dates.date_type \
                    where token is null \
                    and dates.date_type = $1 \
                    and date_types_translations.lang = $2 \
                    order by from_date asc",
                &date_type,
                &lang,
            )
            .fetch_all(&mut **db)
            .await?
            .into_iter()
            .map(|record| Date {
                id: record.id,
                from_date: record.from_date.with_timezone(&Local),
                to_date: record.to_date.with_timezone(&Local),
                room_number: record.room_number,
                date_type: DateType {
                    value: String::from(date_type),
                    display_name: Some(record.display_name),
                },
            })
            .collect(),

            None => sqlx::query!(
                "select dates.id as id, from_date, to_date, room_number, dates.date_type \
                    from dates \
                    join rooms on rooms.id = dates.room_id \
                    left join bookings on dates.id = bookings.date_id \
                    where token is null \
                    and dates.date_type = $1 \
                    order by from_date asc",
                &date_type,
            )
            .fetch_all(&mut **db)
            .await?
            .into_iter()
            .map(|record| Date {
                id: record.id,
                from_date: record.from_date.with_timezone(&Local),
                to_date: record.to_date.with_timezone(&Local),
                room_number: record.room_number,
                date_type: DateType {
                    value: String::from(date_type),
                    display_name: None,
                },
            })
            .collect(),
        };

        if config.days_deadline > 0 {
            let today = datetime_to_day(Local::now());
            dates.retain(|date| {
                datetime_to_day(date.from_date)
                    >= today + Duration::days(config.days_deadline as i64)
            });
        } else {
            let now = Local::now();
            dates.retain(|date| date.from_date >= now);
        }

        if dates.is_empty() || config.dates_per_day == 0 {
            return Ok(dates);
        }

        let mut i = 1;
        let mut current_day = datetime_to_day(dates[0].from_date);
        let mut current_count = 1;
        while i < dates.len() {
            let next_day = datetime_to_day(dates[i].from_date);
            if current_day == next_day {
                if current_count < config.dates_per_day {
                    current_count += 1;
                    i += 1;
                } else {
                    dates.remove(i);
                }
            } else {
                current_count = 1;
                i += 1;
                current_day = next_day;
            }
        }

        Ok(dates)
    }

    pub async fn get_available_date(
        db: &mut Connection<Database>,
        id: i32,
        lang: &str,
        config: &Config,
    ) -> Result<Option<Date>> {
        let date_type = sqlx::query_scalar!("select date_type from dates where id = $1", &id)
            .fetch_one(&mut **db)
            .await?;
        let mut dates = Self::get_available_dates(db, &date_type, config, Some(lang)).await?;
        dates.retain(|date| date.id == id);
        match dates.len() {
            0 => Ok(None),
            1 => Ok(Some(dates.remove(0))),
            _ => Err(anyhow!("IDs of dates are not unique!")),
        }
    }
}
