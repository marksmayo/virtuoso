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

var faker = (function () {
  var first = ['James', 'Mary', 'Robert', 'Patricia', 'John', 'Jennifer', 'Michael', 'Linda', 'David', 'Elizabeth',
    'William', 'Barbara', 'Richard', 'Susan', 'Joseph', 'Jessica', 'Thomas', 'Sarah', 'Aiko', 'Mateo', 'Priya', 'Noah'];
  var last = ['Smith', 'Johnson', 'Williams', 'Brown', 'Jones', 'Garcia', 'Miller', 'Davis', 'Rodriguez', 'Martinez',
    'Hernandez', 'Lopez', 'Gonzalez', 'Wilson', 'Anderson', 'Thomas', 'Taylor', 'Moore', 'Nguyen', 'Kim', 'Patel'];
  var cities = ['Sydney', 'London', 'Paris', 'Tokyo', 'Berlin', 'Toronto', 'Madrid', 'Rome', 'Seoul', 'Lisbon',
    'Dublin', 'Oslo', 'Vienna', 'Prague', 'Auckland', 'Chicago', 'Denver', 'Austin', 'Melbourne', 'Zurich'];
  var countries = ['Australia', 'Canada', 'France', 'Germany', 'Japan', 'Spain', 'Italy', 'Brazil', 'India', 'Norway',
    'Ireland', 'Portugal', 'Mexico', 'Kenya', 'Chile', 'Sweden', 'Austria', 'Peru', 'Egypt', 'Vietnam'];
  var colors = ['red', 'green', 'blue', 'yellow', 'purple', 'orange', 'pink', 'brown', 'black', 'white', 'gray',
    'teal', 'navy', 'maroon', 'olive', 'lime', 'cyan', 'magenta', 'silver', 'gold'];
  var words = ['lorem', 'ipsum', 'dolor', 'sit', 'amet', 'consectetur', 'adipiscing', 'elit', 'sed', 'do', 'eiusmod',
    'tempor', 'incididunt', 'ut', 'labore', 'et', 'dolore', 'magna', 'aliqua', 'enim', 'minim', 'veniam'];
  var domains = ['example.com', 'mail.com', 'test.org', 'inbox.net'];
  function pick(a) { return a[Math.floor(Math.random() * a.length)]; }
  function digits(n) { var s = '' + (1 + Math.floor(Math.random() * 9)); while (s.length < n) s += Math.floor(Math.random() * 10); return s.slice(0, n); }
  function text(n) { var s = ''; while (s.length < n) s += (s ? ' ' : '') + pick(words); return s.slice(0, n); }
  return {
    name: function () {
      return {
        firstName: function () { return pick(first); },
        lastName: function () { return pick(last); },
        fullName: function () { return pick(first) + ' ' + pick(last); },
        name: function () { return pick(first) + ' ' + pick(last); },
      };
    },
    internet: function () {
      return {
        emailAddress: function () { return (pick(first) + '.' + pick(last) + digits(3)).toLowerCase() + '@' + pick(domains); },
        domainName: function () { return pick(domains); },
      };
    },
    number: function () {
      return {
        digits: function (n) { return digits(n || 8); },
        numberBetween: function (a, b) { return a + Math.floor(Math.random() * (b - a)); },
        randomNumber: function () { return Number(digits(8)); },
      };
    },
    address: function () {
      return { city: function () { return pick(cities); }, country: function () { return pick(countries); } };
    },
    color: function () { return { name: function () { return pick(colors); } }; },
    lorem: function () {
      return {
        word: function () { return pick(words); },
        sentence: function () { return text(40) + '.'; },
        characters: function (n) { return text(n || 8).replace(/ /g, 'x'); },
      };
    },
    text: function (n) { return text(n || 8); },
    digits: digits,
  };
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
