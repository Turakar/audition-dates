use anyhow::Result;
use chrono::DateTime;
use chrono::Duration;
use chrono::Local;
use rocket::form::FromFormField;
use rocket::request::FromParam;
use rocket_db_pools::Connection;
use serde::Deserialize;
use serde::Serialize;

use crate::Database;

#[derive(Serialize, Deserialize)]
pub struct Voice {
    value: String,
    display_name: Option<String>,
}

impl Voice {
    pub async fn get_from_value(db: &mut Connection<Database>, value: String, lang: &str) -> Result<Self> {
        let display_name = sqlx::query!(
            "select display_name \
            from voices \
            join voices_translations on voices.id = voices_translations.voice \
            where value = $1 and lang = $2",
        &value, &lang).fetch_one(&mut *db).await?;
        Ok(Voice {
            value,
            display_name: Some(display_name)
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct DateType {
    value: String,
    display_name: Option<String>,
}

impl DateType {
    pub async fn get_variants(db: &mut Connection<Database>, lang: &str) -> Vec<Self> {
        sqlx::query!(
            r#"select id, display_name
            from date_types
            join date_types_translations on date_types.id = date_types_translation.date_type"#)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Date {
    id: i32,
    from_date: DateTime<Local>,
    to_date: DateTime<Local>,
    room_number: String,
    date_type: DateType,
}

impl Date {
    pub async fn get_available_dates(
        db: &mut Connection<Database>,
        date_type: Option<&str>,
        dates_per_day: usize,
        days_deadline: u32,
        lang: Option<&str>,
    ) -> Result<Vec<Date>> {
        let mut dates: Vec<Date> = match (date_type, lang) {
            (Some(date_type), Some(lang)) => sqlx::query!(
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

            (Some(date_type), None) => sqlx::query!(
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

            (None, _) => sqlx::query!(
                "select dates.id as id, from_date, to_date, room_number, date_type \
                    from dates \
                    join rooms on rooms.id = dates.room_id \
                    left join bookings on dates.id = bookings.date_id \
                    where token is null \
                    order by from_date asc",
            )
            .fetch_all(&mut **db)
            .await?
            .into_iter()
            .map(|record| Date {
                id: record.id,
                from_date: record.from_date.with_timezone(&Local),
                to_date: record.to_date.with_timezone(&Local),
                room_number: record.room_number,
                date_type: DateType::from_param(&record.date_type).unwrap(),
            })
            .collect(),
        };

        if days_deadline > 0 {
            let today = Local::today();
            dates.retain(|date| {
                date.from_date.date() >= today + Duration::days(days_deadline as i64)
            });
        } else {
            let now = Local::now();
            dates.retain(|date| date.from_date >= now);
        }

        if dates.is_empty() || dates_per_day == 0 {
            return Ok(dates);
        }

        let mut i = 1;
        let mut current_day = dates[0].from_date.date();
        let mut current_count = 1;
        while i < dates.len() {
            let next_day = dates[i].from_date.date();
            if current_day == next_day {
                if current_count < dates_per_day {
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
        dates_per_day: usize,
        days_deadline: u32,
    ) -> Result<Option<Date>> {
        let record = sqlx::query!(
            "select from_date, to_date, room_number, dates.date_type, display_name \
                from dates \
                join rooms on rooms.id = dates.room_id \
                left join bookings on dates.id = bookings.date_id \
                join date_types_translations on date_types_translations.date_type = dates.date_type \
                where token is null \
                and date_types_translations.lang = $1 \
                order by from_date asc",
                &lang,
    )
        .fetch_optional(&mut **db)
        .await?;
        match record {
            Some(record) => Ok(Some(Date {
                id,
                from_date: record.from_date.with_timezone(&Local),
                to_date: record.to_date.with_timezone(&Local),
                room_number: record.room_number,
                date_type: DateType {
                    value: record.date_type,
                    display_name: Some(record.display_name),
                },
            })),
            None => Ok(None)
        }
        
    }

    pub async fn get_voices(
        &self,
        db: &mut Connection<Database>,
        lang: &str,
        position: &str,
    ) -> Vec<Voice> {
        sqlx::query!(
            "select value, display_name \
        from voices \
        join voices_translations on voices.id = voices_translations.voice \
        where lang = $1 \
        and position::text = $2",
            &lang,
            &position
        )
        .fetch_all(&mut **db)
        .into_iter()
        .map(|record| Voice {
            value: record.value,
            display_name: record.display_name,
        })
        .collect()
    }
}
