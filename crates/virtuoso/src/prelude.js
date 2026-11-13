// What flow scripts see, set up once per engine. __log, __http and __platform come from Rust.
var output = {};
var maestro = { platform: __platform, copiedText: '' };

var console = (function () {
  function show(a) {
    if (typeof a === 'object' && a !== null) {
      try { return JSON.stringify(a); } catch (e) { return String(a); }
    }
    return String(a);
  }
  function log() { __log(Array.prototype.map.call(arguments, show).join(' ')); }
  return { log: log, info: log, warn: log, error: log, debug: log };
})();

function json(text) { return JSON.parse(text); }

function relativePoint(x, y) { return Math.round(x * 100) + '%,' + Math.round(y * 100) + '%'; }

var http = (function () {
  function request(method, url, opts) {
    opts = opts || {};
    var body = opts.body;
    if (body !== undefined && body !== null && typeof body !== 'string') body = JSON.stringify(body);
    var form = opts.multipartForm ? JSON.stringify(opts.multipartForm) : null;
    return JSON.parse(__http(method, url, JSON.stringify(opts.headers || {}), body === undefined ? null : body, form));
  }
  return {
    get: function (url, o) { return request('GET', url, o); },
    post: function (url, o) { return request('POST', url, o); },
    put: function (url, o) { return request('PUT', url, o); },
    delete: function (url, o) { return request('DELETE', url, o); },
    request: function (url, o) { return request((o && o.method) || 'GET', url, o); },
  };
})();

// Datafaker's API (faker.name().firstName()...) for its common providers, drawing from its
// English word lists (datafaker/data.txt, read on first use). Any other provider or method
// is an error naming the call.
var faker = (function () {
  var data = null;
  // A line per list, `name:word|word|...`: no quotes and commas around each word to carry.
  function d() {
    if (!data) {
      data = {};
      __fakerData().split('\n').forEach(function (l) {
        var i = l.indexOf(':');
        if (i > 0) data[l.slice(0, i)] = l.slice(i + 1).split('|');
      });
    }
    return data;
  }
  function int(n) { return Math.floor(Math.random() * n); }
  function pick(a) { return a[int(a.length)]; }
  function list(k) { return function () { return pick(d()[k]); }; }
  function digit() { return '' + int(10); }
  // '#': a digit.
  function numerify(f) { return f.replace(/#/g, digit); }
  function digits(n) { var s = '' + (1 + int(9)); while (s.length < n) s += digit(); return s.slice(0, n); }
  function chars(n, set) { var s = ''; for (var i = 0; i < n; i++) s += set.charAt(int(set.length)); return s; }
  function between(a, b) { return a >= b ? a : a + int(b - a); }
  function lower(s) { return s.toLowerCase().replace(/[^a-z0-9]/g, ''); }
  function cap(s) { return s.charAt(0).toUpperCase() + s.slice(1); }
  function hex(n) { return chars(n, '0123456789abcdef'); }
  function hashes(n) { return new Array(n + 1).join('#'); }
  var LOWER = 'abcdefghijklmnopqrstuvwxyz';
  function first() { return pick(int(2) ? d().male_first_name : d().female_first_name); }
  function last() { return pick(d().last_name); }
  function full() { return first() + ' ' + last(); }
  function username() { return lower(first()) + '.' + lower(last()); }
  function city() {
    switch (int(4)) {
      case 0: return pick(d().city_prefix) + ' ' + first() + pick(d().city_suffix);
      case 1: return pick(d().city_prefix) + ' ' + first();
      case 2: return first() + pick(d().city_suffix);
      default: return last() + pick(d().city_suffix);
    }
  }
  function street() { return (int(2) ? first() : last()) + ' ' + pick(d().street_suffix); }
  function streetAddress() { return numerify(pick(d().building_number)) + ' ' + street(); }
  function zip() { return numerify('#####'); }
  function word() { return pick(d().lorem); }
  function words(n) { var a = []; for (var i = 0; i < n; i++) a.push(word()); return a; }
  function sentence(n) { return cap(words(Math.max(1, (n === undefined ? 3 : n) + int(7))).join(' ')) + '.'; }
  function paragraph(n) { var a = []; for (var i = (n === undefined ? 3 : n) + int(4); i > 0; i--) a.push(sentence()); return a.join(' '); }
  function many(n, f) { var a = []; for (var i = 0; i < (n === undefined ? 3 : n); i++) a.push(f()); return a; }
  function area() { return '' + (2 + int(8)) + digit() + digit(); }
  function phone() { return '(' + area() + ') ' + area() + '-' + numerify('####'); }
  function decimal(a, b) { return a + Math.random() * (b - a); }
  var providers = {
    name: {
      firstName: first, lastName: last, fullName: full, name: full, username: username,
      nameWithMiddle: function () { return first() + ' ' + first() + ' ' + last(); },
      prefix: list('prefix'), suffix: list('suffix'),
    },
    internet: {
      emailAddress: function (local) { return (local === undefined ? username() : '' + local) + '@' + pick(d().free_email); },
      safeEmailAddress: function (local) { return (local === undefined ? username() : '' + local) + '@example.com'; },
      username: username,
      domainWord: function () { return lower(last()); },
      domainName: function () { return lower(last()) + '.' + pick(d().domain_suffix); },
      domainSuffix: list('domain_suffix'),
      url: function () { return 'https://www.' + lower(last()) + '-' + lower(last()) + '.' + pick(d().domain_suffix) + '/'; },
      password: function (min, max) { return chars(between(min === undefined ? 8 : min, (max === undefined ? 16 : max) + 1), LOWER + '0123456789'); },
      uuid: function () { return hex(8) + '-' + hex(4) + '-4' + hex(3) + '-' + '89ab'.charAt(int(4)) + hex(3) + '-' + hex(12); },
      ipV4Address: function () { return [1 + int(254), int(256), int(256), 1 + int(254)].join('.'); },
    },
    address: {
      city: city, cityName: city, cityPrefix: list('city_prefix'), citySuffix: list('city_suffix'),
      country: list('country'), countryCode: list('country_code'),
      streetName: street, streetAddress: streetAddress, streetSuffix: list('street_suffix'),
      buildingNumber: function () { return numerify(pick(d().building_number)); },
      secondaryAddress: function () { return numerify(pick(d().secondary_address)); },
      zipCode: zip, postcode: zip, state: list('state'), stateAbbr: list('state_abbr'),
      fullAddress: function () { return streetAddress() + ', ' + city() + ', ' + pick(d().state_abbr) + ' ' + zip(); },
      latitude: function () { return decimal(-90, 90).toFixed(8); },
      longitude: function () { return decimal(-180, 180).toFixed(8); },
    },
    number: {
      // randomNumber(n) has n digits; without n, up to 10.
      randomNumber: function (n) { return Number(n === undefined ? '' + int(1e10) : digits(Math.max(1, Math.min(18, n)))); },
      randomDigit: function () { return int(10); },
      randomDigitNotZero: function () { return 1 + int(9); },
      numberBetween: function (a, b) { return between(Math.ceil(a), Math.floor(b)); },
      digits: function (n) { return numerify(hashes(n === undefined ? 1 : n)); },
      randomDouble: function (places, a, b) { return Number(decimal(a, b).toFixed(places)); },
      positive: function () { return 1 + int(2147483646); },
      negative: function () { return -(1 + int(2147483647)); },
    },
    lorem: {
      word: word, words: function (n) { return words(n === undefined ? 3 : n); },
      sentence: sentence, paragraph: paragraph,
      sentences: function (n) { return many(n, sentence); },
      paragraphs: function (n) { return many(n, paragraph); },
      characters: function (n) { return chars(n === undefined ? 255 : n, LOWER + '0123456789'); },
    },
    text: {
      text: function (a, b) { return chars(typeof b === 'number' ? between(a, b + 1) : (a === undefined ? 20 : a), LOWER); },
    },
    phoneNumber: {
      cellPhone: phone, phoneNumber: phone,
      subscriberNumber: function (n) { return numerify(hashes(n === undefined ? 4 : n)); },
    },
    color: { name: list('color'), hex: function () { return '#' + hex(6).toUpperCase(); } },
    company: {
      name: function () {
        switch (int(3)) {
          case 0: return last() + ' ' + pick(d().company_suffix);
          case 1: return last() + '-' + last();
          default: return last() + ', ' + last() + ' and ' + last();
        }
      },
      suffix: list('company_suffix'), industry: list('industry'), profession: list('profession'),
    },
    bool: { bool: function () { return int(2) === 1; } },
  };
  // inputRandomText's and inputRandomNumber's own: n letters, n digits.
  function text(n) { return chars(n || 8, LOWER); }
  var faker = { digits: function (n) { return digits(n || 8); } };
  Object.keys(providers).forEach(function (k) {
    // faker.text(n) is inputRandomText's; faker.text() Datafaker's provider.
    faker[k] = function (n) { return k === 'text' && typeof n === 'number' ? text(n) : providers[k]; };
  });
  return faker;
})();

// CommonJS modules. __module(path, fromDir) finds the file (Rust); each loads once.
var require = (function () {
  var cache = {};
  function load(path, from) {
    var m = JSON.parse(__module(path, from));
    if (m.error) throw new Error(m.error);
    if (cache[m.path]) return cache[m.path].exports;
    var module = { exports: {} };
    cache[m.path] = module;
    new Function('module', 'exports', 'require', m.source)(module, module.exports, function (p) { return load(p, m.dir); });
    return module.exports;
  }
  return function (path) { return load(path, ''); };
})();
