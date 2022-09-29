alter table bookings
drop constraint bookings_date_id_fkey,
add constraint bookings_date_id_fkey foreign key (date_id) references dates (id) on delete cascade;

alter table bookings add column lang text;
update bookings set lang = 'de';
alter table bookings alter column lang set not null;
