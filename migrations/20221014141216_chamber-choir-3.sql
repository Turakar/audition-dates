-- create date type

insert into date_types (id)
values ('chamber-choir');

insert into date_types_translations (date_type, lang, display_name)
values ('chamber-choir', 'de', 'Kammerchor'), ('chamber-choir', 'en', 'Chamber choir');

-- create booking voices

with voice as (
	insert into voices (value, date_type, position) values ('soprano', 'chamber-choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Sopran'), ('en', 'Soprano')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('alto', 'chamber-choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Alt'), ('en', 'Alto')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('tenor', 'chamber-choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Tenor'), ('en', 'Tenor')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('bass', 'chamber-choir', 'booking') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Bass'), ('en', 'Bass')) as sub (lang, display_name), voice;

-- create result voices

with voice as (
	insert into voices (value, date_type, position) values ('soprano', 'chamber-choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Sopran'), ('en', 'Soprano')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('alto', 'chamber-choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Alt'), ('en', 'Alto')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('tenor', 'chamber-choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Tenor'), ('en', 'Tenor')) as sub (lang, display_name), voice;

with voice as (
	insert into voices (value, date_type, position) values ('bass', 'chamber-choir', 'result') returning id
)
insert into voices_translations (voice, lang, display_name)
select id, lang, display_name
from (values ('de', 'Bass'), ('en', 'Bass')) as sub (lang, display_name), voice;
