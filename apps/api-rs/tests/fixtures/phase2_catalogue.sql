create table authors (
  id varchar primary key,
  name varchar not null,
  website varchar
);

create table families (
  id varchar primary key,
  name varchar not null,
  pantry_shared boolean not null,
  meal_plan_shared boolean not null
);

create table users (
  id varchar primary key,
  display_name varchar not null,
  email varchar,
  family_id varchar
);

create table cookbooks (
  id varchar primary key,
  title varchar not null,
  isbn varchar,
  publisher varchar,
  published_year integer,
  cover_image_url varchar,
  owner_user_id varchar,
  family_id varchar,
  share_scope varchar
);

create table cookbook_authors (
  cookbook_id varchar not null,
  author_id varchar not null,
  position integer not null,
  primary key (cookbook_id, author_id)
);

create table cookbook_shares (
  cookbook_id varchar not null,
  user_id varchar not null,
  primary key (cookbook_id, user_id)
);

create table recipes (
  id varchar primary key,
  title varchar not null,
  cookbook_id varchar not null,
  source_label varchar not null,
  page_start integer,
  page_end integer,
  yield_quantity double,
  yield_unit varchar,
  prep_minutes integer,
  cook_minutes integer,
  total_minutes integer,
  cuisine varchar,
  category varchar,
  subtitle varchar,
  headnote varchar,
  serving_context varchar,
  source_block_id varchar,
  pictured_page_number integer,
  extraction_status varchar,
  searchable_text varchar not null,
  last_made_at varchar,
  times_made integer not null,
  cost_cents bigint,
  cost_per_serving_cents bigint,
  cache_key varchar not null,
  cache_updated_at varchar
);

create table recipe_authors (
  recipe_id varchar not null,
  author_id varchar not null,
  position integer not null,
  primary key (recipe_id, author_id)
);

create table recipe_tags (
  recipe_id varchar not null,
  tag varchar not null,
  position integer not null,
  primary key (recipe_id, tag)
);

create table recipe_alternate_names (
  recipe_id varchar not null,
  kind varchar not null,
  value varchar not null,
  position integer not null,
  primary key (recipe_id, position)
);

create table recipe_source_page_spans (
  recipe_id varchar not null,
  page_id varchar,
  printed_page_number integer,
  line_start integer,
  line_end integer,
  confidence double,
  position integer not null,
  primary key (recipe_id, position)
);

create table recipe_components (
  recipe_id varchar not null,
  component_recipe_id varchar not null,
  position integer not null,
  primary key (recipe_id, component_recipe_id)
);

create table recipe_ingredients (
  recipe_id varchar not null,
  ingredient_id varchar not null,
  position integer not null,
  display_name varchar not null,
  item varchar not null,
  quantity double,
  quantity_text varchar,
  quantity_min double,
  quantity_max double,
  quantity_kind varchar,
  quantity_review_status varchar,
  quantity_review_reason varchar,
  unit varchar,
  preparation varchar,
  section varchar,
  optional boolean not null,
  alternative_text varchar,
  source_line integer,
  source_page_id varchar,
  unit_cost_cents bigint,
  estimated_cost_cents bigint,
  primary key (recipe_id, ingredient_id)
);

create table recipe_steps (
  recipe_id varchar not null,
  step_id varchar not null,
  position integer not null,
  section varchar,
  text varchar not null,
  source_page_id varchar,
  source_line_start integer,
  source_line_end integer,
  primary key (recipe_id, step_id)
);

create table recipe_images (
  recipe_id varchar not null,
  image_id varchar not null,
  url varchar not null,
  alt varchar not null,
  credit varchar,
  is_primary boolean not null,
  primary key (recipe_id, image_id)
);

create table recipe_notes (
  recipe_id varchar not null,
  note_id varchar not null,
  text varchar not null,
  created_at varchar not null,
  primary key (recipe_id, note_id)
);

create table pantry_items (
  id varchar primary key,
  item varchar not null,
  display_name varchar not null,
  quantity double,
  unit varchar,
  category varchar not null,
  source_recipe_id varchar,
  notes varchar,
  expires_at varchar,
  added_at varchar not null,
  owner_user_id varchar,
  family_id varchar
);

create table meal_plan_entries (
  id varchar primary key,
  date varchar not null,
  meal_type varchar not null,
  recipe_id varchar not null,
  servings double,
  notes varchar,
  owner_user_id varchar,
  family_id varchar
);

create table cook_log_entries (
  id varchar primary key,
  recipe_id varchar not null,
  made_at varchar not null,
  servings_made double,
  servings_eaten double,
  leftover_servings double,
  notes varchar
);

create table cook_log_substitutions (
  id varchar primary key,
  cook_log_id varchar not null,
  ingredient_id varchar not null,
  original_item varchar not null,
  substitute_text varchar not null
);

create table cookbook_imports (
  id varchar primary key,
  cookbook_id varchar not null,
  source_kind varchar not null,
  source_path varchar not null,
  status varchar not null,
  ocr_engine varchar,
  created_at varchar not null,
  updated_at varchar not null,
  review_notes varchar
);

create table cookbook_import_jobs (
  import_id varchar primary key,
  state varchar not null,
  stage varchar not null,
  message varchar not null,
  current_count bigint,
  total_count bigint,
  processed_count bigint not null,
  skipped_count bigint not null,
  failed_count bigint not null,
  section_count bigint not null,
  content_block_count bigint not null,
  recipe_count bigint not null,
  current_section_index bigint,
  section_total bigint,
  current_section_title varchar,
  extraction_engine varchar,
  error_message varchar,
  created_at varchar not null,
  updated_at varchar not null
);

create table recipe_imports (
  id varchar primary key,
  status varchar not null,
  file_name varchar not null,
  mime_type varchar not null,
  image_path varchar not null,
  ocr_engine varchar not null,
  ocr_text varchar not null,
  ocr_json varchar not null,
  draft_json varchar,
  validation_issues_json varchar not null,
  created_at varchar not null,
  updated_at varchar not null
);

create table cookbook_pages (
  id varchar primary key,
  cookbook_id varchar not null,
  import_id varchar not null,
  image_index integer not null,
  printed_page_label varchar,
  printed_page_number integer,
  image_path varchar not null,
  image_hash varchar,
  ocr_text varchar not null,
  ocr_json varchar not null,
  average_confidence double,
  minimum_confidence double,
  page_kind varchar not null,
  review_status varchar not null
);

create table cookbook_sections (
  id varchar primary key,
  cookbook_id varchar not null,
  parent_section_id varchar,
  title varchar not null,
  kind varchar not null,
  position integer not null,
  page_start integer,
  page_end integer
);

create table cookbook_content_blocks (
  id varchar primary key,
  cookbook_id varchar not null,
  section_id varchar,
  page_start integer,
  page_end integer,
  position integer not null,
  kind varchar not null,
  title varchar,
  text varchar not null,
  confidence double,
  source_json varchar not null
);

create table cookbook_menus (
  id varchar primary key,
  cookbook_id varchar not null,
  source_block_id varchar,
  title varchar not null,
  theme varchar,
  notes varchar
);

create table cookbook_menu_recipes (
  menu_id varchar not null,
  recipe_id varchar not null,
  position integer not null,
  role varchar,
  serving_notes varchar,
  primary key (menu_id, recipe_id)
);

create table cookbook_glossary_entries (
  id varchar primary key,
  cookbook_id varchar not null,
  source_block_id varchar,
  title varchar not null,
  aliases_json varchar not null,
  native_names_json varchar not null,
  description varchar not null,
  storage_notes varchar,
  substitution_notes varchar,
  page_start integer,
  page_end integer
);

create table cookbook_suppliers (
  id varchar primary key,
  cookbook_id varchar not null,
  source_block_id varchar,
  name varchar not null,
  url varchar,
  region varchar,
  notes varchar,
  source_page integer,
  review_status varchar not null
);

create table cookbook_index_entries (
  id varchar primary key,
  cookbook_id varchar not null,
  term varchar not null,
  subterm varchar,
  target_page_label varchar,
  target_page_number integer,
  target_recipe_id varchar,
  illustration boolean not null
);

create table cookbook_cross_references (
  id varchar primary key,
  cookbook_id varchar not null,
  from_kind varchar not null,
  from_id varchar not null,
  to_kind varchar not null,
  to_id varchar,
  label varchar,
  relation_kind varchar not null
);

insert into authors values ('author-1', 'Jordan Bourke', 'https://example.test/author');
insert into families values ('river-house', 'River House', true, true);
insert into users values ('avery-river', 'Avery River', 'avery@example.test', 'river-house');
insert into users values ('shared-user', 'Shared User', null, 'river-house');
insert into cookbooks values (
  'our-korean-kitchen', 'Our Korean Kitchen', '9780297609716',
  'Weidenfeld & Nicolson', 2015, '/covers/our-korean-kitchen.jpg',
  'avery-river', 'river-house', 'family'
);
insert into cookbook_authors values ('our-korean-kitchen', 'author-1', 1);
insert into cookbook_shares values ('our-korean-kitchen', 'shared-user');

insert into cookbook_imports values (
  'import-1', 'our-korean-kitchen', 'image_set', 'imports/our-korean-kitchen',
  'committed', 'paddleocr:3.7.0:paddleocr3',
  '2026-07-09T00:00:00Z', '2026-07-09T01:00:00Z', null
);
insert into cookbook_pages values (
  'page-26', 'our-korean-kitchen', 'import-1', 4, '26', 26,
  '/tmp/recitopia-phase2-page.jpg', 'phase2-image-hash',
  repeat('x', 430) || 'é', '{"blocks":[{"text":"Short-grain Rice"}]}',
  0.97, 0.72, 'recipe', 'accepted'
);
insert into cookbook_sections values (
  'section-1', 'our-korean-kitchen', null, 'Rice & Savoury Porridge',
  'chapter', 1, 25, 51
);
insert into cookbook_content_blocks values (
  'block-1', 'our-korean-kitchen', 'section-1', 26, 26, 1,
  'recipe_headnote', 'Short-grain Rice', repeat('b', 430), 0.93,
  '{"sourcePageIds":["page-26"]}'
);

insert into recipes values (
  'recipe-1', 'Short-grain Rice', 'our-korean-kitchen',
  'Our Korean Kitchen, p. 26', 26, 26, 6, 'servings', 30, 30, 60,
  'Korean', 'Rice', 'Bap', 'The everyday Korean rice.',
  'Serve with soup and banchan.', 'block-1', 27, 'needs_review',
  'short grain rice bap korean', '2026-07-08T18:00:00Z', 2,
  450, 75, 'recipe-cache-1', '2026-07-09T01:00:00Z'
);
insert into recipe_authors values ('recipe-1', 'author-1', 1);
insert into recipe_tags values ('recipe-1', 'rice', 1);
insert into recipe_tags values ('recipe-1', 'staple', 2);
insert into recipe_alternate_names values ('recipe-1', 'romanized', 'bap', 1);
insert into recipe_source_page_spans values ('recipe-1', 'page-26', 26, 1, 35, 0.96, 1);
insert into recipe_components values ('recipe-1', 'component-stock', 1);
insert into recipe_ingredients values (
  'recipe-1', 'ingredient-1', 1, '?00g short-grain white rice',
  'short-grain white rice', null, '?00g', null, null, 'unknown',
  'needs_review', 'OCR may have lost a leading digit', 'g', 'rinsed',
  'Short-grain Rice', false, null, 4, 'page-26', 2, null
);
insert into recipe_steps values (
  'recipe-1', 'step-1', 1, 'Short-grain Rice',
  'Rinse and drain the rice three times.', 'page-26', 12, 14
);
insert into recipe_images values (
  'recipe-1', 'image-1', '/api/cookbook-pages/page-26/image',
  'A bowl of short-grain rice', 'Tara Fisher', true
);
insert into recipe_notes values (
  'recipe-1', 'note-1', 'Use the smallest heavy pan.', '2026-07-09T02:00:00Z'
);

insert into pantry_items values (
  'pantry-1', 'short-grain white rice', 'Short-grain white rice', 1.5,
  'kg', 'raw', 'recipe-1', 'Keep sealed', null,
  '2026-07-09T02:00:00Z', 'avery-river', 'river-house'
);
insert into meal_plan_entries values (
  'meal-plan-initial', '2026-07-11', 'dinner', 'recipe-1', 4,
  'Serve with kimchi', 'avery-river', 'river-house'
);
insert into cook_log_entries values (
  'cook-log-initial', 'recipe-1', '2026-07-08T18:00:00Z', 6, 4, 2,
  'Rice was excellent'
);
insert into cook_log_substitutions values (
  'sub-initial', 'cook-log-initial', 'ingredient-1',
  'short-grain white rice', 'brown short-grain rice'
);

insert into cookbook_menus values (
  'menu-1', 'our-korean-kitchen', 'block-1', 'Weeknight menu',
  'Comfort food', 'Serve everything together'
);
insert into cookbook_menu_recipes values (
  'menu-1', 'recipe-1', 1, 'main', 'Serve warm'
);
insert into cookbook_glossary_entries values (
  'glossary-1', 'our-korean-kitchen', 'block-1', 'Gochujang',
  '["chilli paste"]', '["gochujang"]', 'A fermented chilli paste.',
  'Refrigerate after opening', 'Use doenjang for a different flavour', 13, 14
);
insert into cookbook_suppliers values (
  'supplier-1', 'our-korean-kitchen', 'block-1', 'Korean Foods',
  'https://example.test/shop', 'London', 'Online ordering', 265, 'accepted'
);
insert into cookbook_index_entries values (
  'index-1', 'our-korean-kitchen', 'rice', 'short-grain', '26', 26,
  'recipe-1', false
);
insert into cookbook_cross_references values (
  'xref-1', 'our-korean-kitchen', 'recipe', 'recipe-1', 'page',
  'page-26', 'See the grain guide', 'see_also'
);
