alter table waiting_list
add column token text primary key default gen_random_uuid(),
drop column id;
