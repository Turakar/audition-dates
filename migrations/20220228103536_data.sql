create table rooms (
    id serial primary key,
    room_number text not null unique
);

create table dates (
    id serial primary key,
    from_date timestamp with time zone not null,
    to_date timestamp with time zone not null,
    room_id integer references rooms (id),
    date_type text not null,
    active boolean not null default false
);

create table bookings (
    token text primary key default gen_random_uuid(),
    date_id integer references dates (id),
    email text not null unique,
    person_name text not null,
    notes text not null,
    voice text not null
);
